#![allow(clippy::implicit_hasher)]
use {
    crate::shred,
    itertools::Itertools,
    rayon::{prelude::*, ThreadPool},
    sha2::{Digest, Sha512},
    solana_metrics::inc_new_counter_debug,
    solana_perf::{
        cuda_runtime::PinnedVec,
        packet::{Packet, PacketBatch},
        perf_libs,
        recycler_cache::RecyclerCache,
        sigverify::{self, count_packets_in_batches, TxOffset},
    },
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::{
        clock::Slot,
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
    },
    std::{collections::HashMap, fmt::Debug, iter::repeat, mem::size_of, ops::Range, sync::Arc},
};

const SIGN_SHRED_GPU_MIN: usize = 256;

lazy_static! {
    static ref SIGVERIFY_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_thread_count())
        .thread_name(|ix| format!("sigverify_shreds_{}", ix))
        .build()
        .unwrap();
}

pub fn verify_shred_cpu(
    packet: &Packet,
    slot_leaders: &HashMap<Slot, /*pubkey:*/ [u8; 32]>,
) -> bool {
    if packet.meta.discard() {
        return false;
    }
    let shred = match shred::layout::get_shred(packet) {
        None => return false,
        Some(shred) => shred,
    };
    let slot = match shred::layout::get_slot(shred) {
        None => return false,
        Some(slot) => slot,
    };
    trace!("slot {}", slot);
    let pubkey = match slot_leaders.get(&slot) {
        None => return false,
        Some(pubkey) => pubkey,
    };
    let signature = match shred::layout::get_signature(shred) {
        None => return false,
        Some(signature) => signature,
    };
    trace!("signature {}", signature);
    let message =
        match shred::layout::get_signed_message_range(shred).and_then(|slice| shred.get(slice)) {
            None => return false,
            Some(message) => message,
        };
    signature.verify(pubkey, message)
}

fn verify_shreds_cpu(
    batches: &[PacketBatch],
    slot_leaders: &HashMap<Slot, /*pubkey:*/ [u8; 32]>,
) -> Vec<Vec<u8>> {
    let packet_count = count_packets_in_batches(batches);
    debug!("CPU SHRED ECDSA for {}", packet_count);
    let rv = SIGVERIFY_THREAD_POOL.install(|| {
        batches
            .into_par_iter()
            .map(|batch| {
                batch
                    .par_iter()
                    .map(|packet| u8::from(verify_shred_cpu(packet, slot_leaders)))
                    .collect()
            })
            .collect()
    });
    inc_new_counter_debug!("ed25519_shred_verify_cpu", packet_count);
    rv
}

fn slot_key_data_for_gpu<T>(
    offset_start: usize,
    batches: &[PacketBatch],
    slot_keys: &HashMap<Slot, /*pubkey:*/ T>,
    recycler_cache: &RecyclerCache,
) -> (PinnedVec<u8>, TxOffset, usize)
where
    T: AsRef<[u8]> + Copy + Debug + Default + Eq + std::hash::Hash + Sync,
{
    //TODO: mark Pubkey::default shreds as failed after the GPU returns
    assert_eq!(slot_keys.get(&Slot::MAX), Some(&T::default()));
    let slots: Vec<Slot> = SIGVERIFY_THREAD_POOL.install(|| {
        batches
            .into_par_iter()
            .flat_map_iter(|batch| {
                batch.iter().map(|packet| {
                    if packet.meta.discard() {
                        return Slot::MAX;
                    }
                    let shred = shred::layout::get_shred(packet);
                    match shred.and_then(shred::layout::get_slot) {
                        Some(slot) if slot_keys.contains_key(&slot) => slot,
                        _ => Slot::MAX,
                    }
                })
            })
            .collect()
    });
    let keys_to_slots: HashMap<T, Vec<Slot>> = slots
        .iter()
        .map(|slot| (*slot_keys.get(slot).unwrap(), *slot))
        .into_group_map();
    let mut keyvec = recycler_cache.buffer().allocate("shred_gpu_pubkeys");
    keyvec.set_pinnable();

    let keyvec_size = keys_to_slots.len() * size_of::<T>();
    keyvec.resize(keyvec_size, 0);

    let slot_to_key_ix: HashMap<Slot, /*key index:*/ usize> = keys_to_slots
        .into_iter()
        .enumerate()
        .flat_map(|(i, (k, slots))| {
            let start = i * size_of::<T>();
            let end = start + size_of::<T>();
            keyvec[start..end].copy_from_slice(k.as_ref());
            slots.into_iter().zip(repeat(i))
        })
        .collect();

    let mut offsets = recycler_cache.offsets().allocate("shred_offsets");
    offsets.set_pinnable();
    for slot in slots {
        let key_offset = slot_to_key_ix.get(&slot).unwrap() * size_of::<T>();
        offsets.push((offset_start + key_offset) as u32);
    }
    let num_in_packets = resize_vec(&mut keyvec);
    trace!("keyvec.len: {}", keyvec.len());
    trace!("keyvec: {:?}", keyvec);
    trace!("offsets: {:?}", offsets);
    (keyvec, offsets, num_in_packets)
}

fn vec_size_in_packets(keyvec: &PinnedVec<u8>) -> usize {
    (keyvec.len() + (size_of::<Packet>() - 1)) / size_of::<Packet>()
}

fn resize_vec(keyvec: &mut PinnedVec<u8>) -> usize {
    //HACK: Pubkeys vector is passed along as a `PacketBatch` buffer to the GPU
    //TODO: GPU needs a more opaque interface, which can handle variable sized structures for data
    //Pad the Pubkeys buffer such that it is bigger than a buffer of Packet sized elems
    let num_in_packets = (keyvec.len() + (size_of::<Packet>() - 1)) / size_of::<Packet>();
    keyvec.resize(num_in_packets * size_of::<Packet>(), 0u8);
    num_in_packets
}

fn shred_gpu_offsets(
    mut pubkeys_end: usize,
    batches: &[PacketBatch],
    recycler_cache: &RecyclerCache,
) -> (TxOffset, TxOffset, TxOffset, Vec<Vec<u32>>) {
    fn add_offset(range: Range<usize>, offset: usize) -> Range<usize> {
        range.start + offset..range.end + offset
    }
    let mut signature_offsets = recycler_cache.offsets().allocate("shred_signatures");
    signature_offsets.set_pinnable();
    let mut msg_start_offsets = recycler_cache.offsets().allocate("shred_msg_starts");
    msg_start_offsets.set_pinnable();
    let mut msg_sizes = recycler_cache.offsets().allocate("shred_msg_sizes");
    msg_sizes.set_pinnable();
    let mut v_sig_lens = vec![];
    for batch in batches.iter() {
        let mut sig_lens = Vec::new();
        for packet in batch.iter() {
            let sig = shred::layout::get_signature_range();
            let sig = add_offset(sig, pubkeys_end);
            debug_assert_eq!(sig.end - sig.start, std::mem::size_of::<Signature>());
            let shred = shred::layout::get_shred(packet);
            // Signature may verify for an empty message but the packet will be
            // discarded during deserialization.
            let msg = shred.and_then(shred::layout::get_signed_message_range);
            let msg = add_offset(msg.unwrap_or_default(), pubkeys_end);
            signature_offsets.push(sig.start as u32);
            msg_start_offsets.push(msg.start as u32);
            let msg_size = msg.end.saturating_sub(msg.start);
            msg_sizes.push(msg_size as u32);
            sig_lens.push(1);
            pubkeys_end += size_of::<Packet>();
        }
        v_sig_lens.push(sig_lens);
    }
    (signature_offsets, msg_start_offsets, msg_sizes, v_sig_lens)
}

pub fn verify_shreds_gpu(
    batches: &[PacketBatch],
    slot_leaders: &HashMap<Slot, /*pubkey:*/ [u8; 32]>,
    recycler_cache: &RecyclerCache,
) -> Vec<Vec<u8>> {
    let api = perf_libs::api();
    if api.is_none() {
        return verify_shreds_cpu(batches, slot_leaders);
    }
    let api = api.unwrap();

    let mut elems = Vec::new();
    let mut rvs = Vec::new();
    let packet_count = count_packets_in_batches(batches);
    let (pubkeys, pubkey_offsets, mut num_packets) =
        slot_key_data_for_gpu(0, batches, slot_leaders, recycler_cache);
    //HACK: Pubkeys vector is passed along as a `PacketBatch` buffer to the GPU
    //TODO: GPU needs a more opaque interface, which can handle variable sized structures for data
    let pubkeys_len = num_packets * size_of::<Packet>();
    trace!("num_packets: {}", num_packets);
    trace!("pubkeys_len: {}", pubkeys_len);
    let (signature_offsets, msg_start_offsets, msg_sizes, v_sig_lens) =
        shred_gpu_offsets(pubkeys_len, batches, recycler_cache);
    let mut out = recycler_cache.buffer().allocate("out_buffer");
    out.set_pinnable();
    elems.push(perf_libs::Elems {
        #[allow(clippy::cast_ptr_alignment)]
        elems: pubkeys.as_ptr() as *const solana_sdk::packet::Packet,
        num: num_packets as u32,
    });

    for batch in batches {
        elems.push(perf_libs::Elems {
            elems: batch.as_ptr(),
            num: batch.len() as u32,
        });
        let mut v = Vec::new();
        v.resize(batch.len(), 0);
        rvs.push(v);
        num_packets += batch.len();
    }
    out.resize(signature_offsets.len(), 0);

    trace!("Starting verify num packets: {}", num_packets);
    trace!("elem len: {}", elems.len() as u32);
    trace!("packet sizeof: {}", size_of::<Packet>() as u32);
    const USE_NON_DEFAULT_STREAM: u8 = 1;
    unsafe {
        let res = (api.ed25519_verify_many)(
            elems.as_ptr(),
            elems.len() as u32,
            size_of::<Packet>() as u32,
            num_packets as u32,
            signature_offsets.len() as u32,
            msg_sizes.as_ptr(),
            pubkey_offsets.as_ptr(),
            signature_offsets.as_ptr(),
            msg_start_offsets.as_ptr(),
            out.as_mut_ptr(),
            USE_NON_DEFAULT_STREAM,
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done verify");
    trace!("out buf {:?}", out);

    sigverify::copy_return_values(&v_sig_lens, &out, &mut rvs);

    inc_new_counter_debug!("ed25519_shred_verify_gpu", packet_count);
    rvs
}

fn sign_shred_cpu(keypair: &Keypair, packet: &mut Packet) {
    let sig = shred::layout::get_signature_range();
    let msg = shred::layout::get_shred(packet)
        .and_then(shred::layout::get_signed_message_range)
        .unwrap();
    assert!(
        packet.meta.size >= sig.end,
        "packet is not large enough for a signature"
    );
    let signature = keypair.sign_message(packet.data(msg).unwrap());
    trace!("signature {:?}", signature);
    packet.buffer_mut()[sig].copy_from_slice(signature.as_ref());
}

pub fn sign_shreds_cpu(keypair: &Keypair, batches: &mut [PacketBatch]) {
    let packet_count = count_packets_in_batches(batches);
    debug!("CPU SHRED ECDSA for {}", packet_count);
    SIGVERIFY_THREAD_POOL.install(|| {
        batches.par_iter_mut().for_each(|batch| {
            batch[..]
                .par_iter_mut()
                .for_each(|p| sign_shred_cpu(keypair, p));
        });
    });
    inc_new_counter_debug!("ed25519_shred_verify_cpu", packet_count);
}

pub fn sign_shreds_gpu_pinned_keypair(keypair: &Keypair, cache: &RecyclerCache) -> PinnedVec<u8> {
    let mut vec = cache.buffer().allocate("pinned_keypair");
    let pubkey = keypair.pubkey().to_bytes();
    let secret = keypair.secret().to_bytes();
    let mut hasher = Sha512::default();
    hasher.update(&secret);
    let mut result = hasher.finalize();
    result[0] &= 248;
    result[31] &= 63;
    result[31] |= 64;
    vec.resize(pubkey.len() + result.len(), 0);
    vec[0..pubkey.len()].copy_from_slice(&pubkey);
    vec[pubkey.len()..].copy_from_slice(&result);
    resize_vec(&mut vec);
    vec
}

pub fn sign_shreds_gpu(
    keypair: &Keypair,
    pinned_keypair: &Option<Arc<PinnedVec<u8>>>,
    batches: &mut [PacketBatch],
    recycler_cache: &RecyclerCache,
) {
    let sig_size = size_of::<Signature>();
    let pubkey_size = size_of::<Pubkey>();
    let api = perf_libs::api();
    let packet_count = count_packets_in_batches(batches);
    if api.is_none() || packet_count < SIGN_SHRED_GPU_MIN || pinned_keypair.is_none() {
        return sign_shreds_cpu(keypair, batches);
    }
    let api = api.unwrap();
    let pinned_keypair = pinned_keypair.as_ref().unwrap();

    let mut elems = Vec::new();
    let offset: usize = pinned_keypair.len();
    let num_keypair_packets = vec_size_in_packets(pinned_keypair);
    let mut num_packets = num_keypair_packets;

    //should be zero
    let mut pubkey_offsets = recycler_cache.offsets().allocate("pubkey offsets");
    pubkey_offsets.resize(packet_count, 0);

    let mut secret_offsets = recycler_cache.offsets().allocate("secret_offsets");
    secret_offsets.resize(packet_count, pubkey_size as u32);

    trace!("offset: {}", offset);
    let (signature_offsets, msg_start_offsets, msg_sizes, _v_sig_lens) =
        shred_gpu_offsets(offset, batches, recycler_cache);
    let total_sigs = signature_offsets.len();
    let mut signatures_out = recycler_cache.buffer().allocate("ed25519 signatures");
    signatures_out.set_pinnable();
    signatures_out.resize(total_sigs * sig_size, 0);
    elems.push(perf_libs::Elems {
        #[allow(clippy::cast_ptr_alignment)]
        elems: pinned_keypair.as_ptr() as *const solana_sdk::packet::Packet,
        num: num_keypair_packets as u32,
    });

    for batch in batches.iter() {
        elems.push(perf_libs::Elems {
            elems: batch.as_ptr(),
            num: batch.len() as u32,
        });
        let mut v = Vec::new();
        v.resize(batch.len(), 0);
        num_packets += batch.len();
    }

    trace!("Starting verify num packets: {}", num_packets);
    trace!("elem len: {}", elems.len() as u32);
    trace!("packet sizeof: {}", size_of::<Packet>() as u32);
    const USE_NON_DEFAULT_STREAM: u8 = 1;
    unsafe {
        let res = (api.ed25519_sign_many)(
            elems.as_mut_ptr(),
            elems.len() as u32,
            size_of::<Packet>() as u32,
            num_packets as u32,
            total_sigs as u32,
            msg_sizes.as_ptr(),
            pubkey_offsets.as_ptr(),
            secret_offsets.as_ptr(),
            msg_start_offsets.as_ptr(),
            signatures_out.as_mut_ptr(),
            USE_NON_DEFAULT_STREAM,
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done sign");
    let mut sizes: Vec<usize> = vec![0];
    sizes.extend(batches.iter().map(|b| b.len()));
    for i in 0..sizes.len() {
        if i == 0 {
            continue;
        }
        sizes[i] += sizes[i - 1];
    }
    SIGVERIFY_THREAD_POOL.install(|| {
        batches
            .par_iter_mut()
            .enumerate()
            .for_each(|(batch_ix, batch)| {
                let num_packets = sizes[batch_ix];
                batch[..]
                    .par_iter_mut()
                    .enumerate()
                    .for_each(|(packet_ix, packet)| {
                        let sig_ix = packet_ix + num_packets;
                        let sig_start = sig_ix * sig_size;
                        let sig_end = sig_start + sig_size;
                        packet.buffer_mut()[..sig_size]
                            .copy_from_slice(&signatures_out[sig_start..sig_end]);
                    });
            });
    });
    inc_new_counter_debug!("ed25519_shred_sign_gpu", packet_count);
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::shred::{Shred, ShredFlags, LEGACY_SHRED_DATA_CAPACITY},
        solana_sdk::signature::{Keypair, Signer},
    };

    fn run_test_sigverify_shred_cpu(slot: Slot) {
        solana_logger::setup();
        let mut packet = Packet::default();
        let mut shred = Shred::new_from_data(
            slot,
            0xc0de,
            0xdead,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        assert_eq!(shred.slot(), slot);
        let keypair = Keypair::new();
        shred.sign(&keypair);
        trace!("signature {}", shred.signature());
        packet.buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        packet.meta.size = shred.payload().len();

        let leader_slots = [(slot, keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        assert!(verify_shred_cpu(&packet, &leader_slots));

        let wrong_keypair = Keypair::new();
        let leader_slots = [(slot, wrong_keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        assert!(!verify_shred_cpu(&packet, &leader_slots));

        let leader_slots = HashMap::new();
        assert!(!verify_shred_cpu(&packet, &leader_slots));
    }

    #[test]
    fn test_sigverify_shred_cpu() {
        run_test_sigverify_shred_cpu(0xdead_c0de);
    }

    fn run_test_sigverify_shreds_cpu(slot: Slot) {
        solana_logger::setup();
        let mut batches = [PacketBatch::default()];
        let mut shred = Shred::new_from_data(
            slot,
            0xc0de,
            0xdead,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        let keypair = Keypair::new();
        shred.sign(&keypair);
        batches[0].resize(1, Packet::default());
        batches[0][0].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][0].meta.size = shred.payload().len();

        let leader_slots = [(slot, keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shreds_cpu(&batches, &leader_slots);
        assert_eq!(rv, vec![vec![1]]);

        let wrong_keypair = Keypair::new();
        let leader_slots = [(slot, wrong_keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shreds_cpu(&batches, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = HashMap::new();
        let rv = verify_shreds_cpu(&batches, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = [(slot, keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        batches[0][0].meta.size = 0;
        let rv = verify_shreds_cpu(&batches, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);
    }

    #[test]
    fn test_sigverify_shreds_cpu() {
        run_test_sigverify_shreds_cpu(0xdead_c0de);
    }

    fn run_test_sigverify_shreds_gpu(slot: Slot) {
        solana_logger::setup();
        let recycler_cache = RecyclerCache::default();

        let mut batches = [PacketBatch::default()];
        let mut shred = Shred::new_from_data(
            slot,
            0xc0de,
            0xdead,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        let keypair = Keypair::new();
        shred.sign(&keypair);
        batches[0].resize(1, Packet::default());
        batches[0][0].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][0].meta.size = shred.payload().len();

        let leader_slots = [
            (std::u64::MAX, Pubkey::default().to_bytes()),
            (slot, keypair.pubkey().to_bytes()),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(&batches, &leader_slots, &recycler_cache);
        assert_eq!(rv, vec![vec![1]]);

        let wrong_keypair = Keypair::new();
        let leader_slots = [
            (std::u64::MAX, Pubkey::default().to_bytes()),
            (slot, wrong_keypair.pubkey().to_bytes()),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(&batches, &leader_slots, &recycler_cache);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = [(std::u64::MAX, [0u8; 32])].iter().cloned().collect();
        let rv = verify_shreds_gpu(&batches, &leader_slots, &recycler_cache);
        assert_eq!(rv, vec![vec![0]]);

        batches[0][0].meta.size = 0;
        let leader_slots = [
            (std::u64::MAX, Pubkey::default().to_bytes()),
            (slot, keypair.pubkey().to_bytes()),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(&batches, &leader_slots, &recycler_cache);
        assert_eq!(rv, vec![vec![0]]);
    }

    #[test]
    fn test_sigverify_shreds_gpu() {
        run_test_sigverify_shreds_gpu(0xdead_c0de);
    }

    fn run_test_sigverify_shreds_sign_gpu(slot: Slot) {
        solana_logger::setup();
        let recycler_cache = RecyclerCache::default();

        let num_packets = 32;
        let num_batches = 100;
        let mut packet_batch = PacketBatch::with_capacity(num_packets);
        packet_batch.resize(num_packets, Packet::default());

        for (i, p) in packet_batch.iter_mut().enumerate() {
            let shred = Shred::new_from_data(
                slot,
                0xc0de,
                i as u16,
                &[5; LEGACY_SHRED_DATA_CAPACITY],
                ShredFlags::LAST_SHRED_IN_SLOT,
                1,
                2,
                0xc0de,
            );
            shred.copy_to_packet(p);
        }
        let mut batches = vec![packet_batch; num_batches];
        let keypair = Keypair::new();
        let pinned_keypair = sign_shreds_gpu_pinned_keypair(&keypair, &recycler_cache);
        let pinned_keypair = Some(Arc::new(pinned_keypair));
        let pubkeys = [
            (std::u64::MAX, Pubkey::default().to_bytes()),
            (slot, keypair.pubkey().to_bytes()),
        ]
        .iter()
        .cloned()
        .collect();
        //unsigned
        let rv = verify_shreds_gpu(&batches, &pubkeys, &recycler_cache);
        assert_eq!(rv, vec![vec![0; num_packets]; num_batches]);
        //signed
        sign_shreds_gpu(&keypair, &pinned_keypair, &mut batches, &recycler_cache);
        let rv = verify_shreds_cpu(&batches, &pubkeys);
        assert_eq!(rv, vec![vec![1; num_packets]; num_batches]);

        let rv = verify_shreds_gpu(&batches, &pubkeys, &recycler_cache);
        assert_eq!(rv, vec![vec![1; num_packets]; num_batches]);
    }

    #[test]
    fn test_sigverify_shreds_sign_gpu() {
        run_test_sigverify_shreds_sign_gpu(0xdead_c0de);
    }

    fn run_test_sigverify_shreds_sign_cpu(slot: Slot) {
        solana_logger::setup();

        let mut batches = [PacketBatch::default()];
        let keypair = Keypair::new();
        let shred = Shred::new_from_data(
            slot,
            0xc0de,
            0xdead,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        batches[0].resize(1, Packet::default());
        batches[0][0].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][0].meta.size = shred.payload().len();

        let pubkeys = [
            (slot, keypair.pubkey().to_bytes()),
            (std::u64::MAX, Pubkey::default().to_bytes()),
        ]
        .iter()
        .cloned()
        .collect();
        //unsigned
        let rv = verify_shreds_cpu(&batches, &pubkeys);
        assert_eq!(rv, vec![vec![0]]);
        //signed
        sign_shreds_cpu(&keypair, &mut batches);
        let rv = verify_shreds_cpu(&batches, &pubkeys);
        assert_eq!(rv, vec![vec![1]]);
    }

    #[test]
    fn test_sigverify_shreds_sign_cpu() {
        run_test_sigverify_shreds_sign_cpu(0xdead_c0de);
    }
}
