#![allow(clippy::implicit_hasher)]
use crate::shred::ShredType;
use rayon::iter::IndexedParallelIterator;
use rayon::iter::IntoParallelIterator;
use rayon::iter::IntoParallelRefMutIterator;
use rayon::iter::ParallelIterator;
use rayon::ThreadPool;
use sha2::{Digest, Sha512};
use solana_ed25519_dalek::{Keypair, PublicKey, SecretKey};
use solana_metrics::inc_new_counter_debug;
use solana_perf::cuda_runtime::PinnedVec;
use solana_perf::packet::{limited_deserialize, Packet, Packets};
use solana_perf::perf_libs;
use solana_perf::recycler::Recycler;
use solana_perf::sigverify::{self, TxOffset};
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::signature::Signature;
use std::collections::HashMap;
use std::mem::size_of;

use std::cell::RefCell;

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .thread_name(|ix| format!("sigverify_shreds_{}", ix))
                    .build()
                    .unwrap()));

/// Assuming layout is
/// signature: Signature
/// signed_msg: {
///   type: ShredType
///   slot: u64,
///   ...
/// }
/// Signature is the first thing in the packet, and slot is the first thing in the signed message.
fn verify_shred_cpu(packet: &Packet, slot_leaders: &HashMap<u64, [u8; 32]>) -> Option<u8> {
    let sig_start = 0;
    let sig_end = size_of::<Signature>();
    let slot_start = sig_end + size_of::<ShredType>();
    let slot_end = slot_start + size_of::<u64>();
    let msg_start = sig_end;
    let msg_end = packet.meta.size;
    trace!("slot start and end {} {}", slot_start, slot_end);
    if packet.meta.size < slot_end {
        return Some(0);
    }
    let slot: u64 = limited_deserialize(&packet.data[slot_start..slot_end]).ok()?;
    trace!("slot {}", slot);
    let pubkey = slot_leaders.get(&slot)?;
    if packet.meta.size < sig_end {
        return Some(0);
    }
    let signature = Signature::new(&packet.data[sig_start..sig_end]);
    trace!("signature {}", signature);
    if !signature.verify(pubkey, &packet.data[msg_start..msg_end]) {
        return Some(0);
    }
    Some(1)
}

fn verify_shreds_cpu(batches: &[Packets], slot_leaders: &HashMap<u64, [u8; 32]>) -> Vec<Vec<u8>> {
    use rayon::prelude::*;
    let count = sigverify::batch_size(batches);
    debug!("CPU SHRED ECDSA for {}", count);
    let rv = PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            batches
                .into_par_iter()
                .map(|p| {
                    p.packets
                        .iter()
                        .map(|p| verify_shred_cpu(p, slot_leaders).unwrap_or(0))
                        .collect()
                })
                .collect()
        })
    });
    inc_new_counter_debug!("ed25519_shred_verify_cpu", count);
    rv
}

fn slot_key_data_for_gpu<
    T: Sync + Sized + Default + std::fmt::Debug + Eq + std::hash::Hash + Clone + Copy + AsRef<[u8]>,
>(
    offset_start: usize,
    batches: &[Packets],
    slot_keys: &HashMap<u64, T>,
    recycler_offsets: &Recycler<TxOffset>,
    recycler_keys: &Recycler<PinnedVec<u8>>,
) -> (PinnedVec<u8>, TxOffset, usize) {
    //TODO: mark Pubkey::default shreds as failed after the GPU returns
    assert_eq!(slot_keys.get(&std::u64::MAX), Some(&T::default()));
    let slots: Vec<Vec<u64>> = PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            batches
                .into_par_iter()
                .map(|p| {
                    p.packets
                        .iter()
                        .map(|packet| {
                            let slot_start = size_of::<Signature>() + size_of::<ShredType>();
                            let slot_end = slot_start + size_of::<u64>();
                            if packet.meta.size < slot_end {
                                return std::u64::MAX;
                            }
                            let slot: Option<u64> =
                                limited_deserialize(&packet.data[slot_start..slot_end]).ok();
                            match slot {
                                Some(slot) if slot_keys.get(&slot).is_some() => slot,
                                _ => std::u64::MAX,
                            }
                        })
                        .collect()
                })
                .collect()
        })
    });
    let mut keys_to_slots: HashMap<T, Vec<u64>> = HashMap::new();
    for batch in slots.iter() {
        for slot in batch.iter() {
            let key = slot_keys.get(slot).unwrap();
            keys_to_slots
                .entry(*key)
                .or_insert_with(|| vec![])
                .push(*slot);
        }
    }
    let mut keyvec = recycler_keys.allocate("shred_gpu_pubkeys");
    let mut slot_to_key_ix = HashMap::new();
    for (i, (k, slots)) in keys_to_slots.iter().enumerate() {
        keyvec.extend(k.as_ref());
        for s in slots {
            slot_to_key_ix.insert(s, i);
        }
    }
    let mut offsets = recycler_offsets.allocate("shred_offsets");
    slots.iter().for_each(|packet_slots| {
        packet_slots.iter().for_each(|slot| {
            offsets
                .push((offset_start + (slot_to_key_ix.get(slot).unwrap() * size_of::<T>())) as u32);
        });
    });
    //HACK: Pubkeys vector is passed along as a `Packets` buffer to the GPU
    //TODO: GPU needs a more opaque interface, which can handle variable sized structures for data
    //Pad the Pubkeys buffer such that it is bigger than a buffer of Packet sized elems
    let num_in_packets = (keyvec.len() + (size_of::<Packet>() - 1)) / size_of::<Packet>();
    trace!("num_in_packets {}", num_in_packets);
    //number of bytes missing
    let missing = num_in_packets * size_of::<Packet>() - keyvec.len();
    trace!("missing {}", missing);
    //extra Pubkeys needed to fill the buffer
    let extra = (missing + size_of::<T>() - 1) / size_of::<T>();
    trace!("extra {}", extra);
    trace!("keyvec {}", keyvec.len());
    keyvec.resize(keyvec.len() + extra, 0u8);
    trace!("keyvec {}", keyvec.len());
    trace!("keyvec {:?}", keyvec);
    trace!("offsets {:?}", offsets);
    (keyvec, offsets, num_in_packets)
}

fn shred_gpu_offsets(
    mut pubkeys_end: usize,
    batches: &[Packets],
    recycler_offsets: &Recycler<TxOffset>,
) -> (TxOffset, TxOffset, TxOffset, Vec<Vec<u32>>) {
    let mut signature_offsets = recycler_offsets.allocate("shred_signatures");
    let mut msg_start_offsets = recycler_offsets.allocate("shred_msg_starts");
    let mut msg_sizes = recycler_offsets.allocate("shred_msg_sizes");
    let mut v_sig_lens = vec![];
    for batch in batches {
        let mut sig_lens = Vec::new();
        for packet in &batch.packets {
            let sig_start = pubkeys_end;
            let sig_end = sig_start + size_of::<Signature>();
            let msg_start = sig_end;
            let msg_end = sig_start + packet.meta.size;
            signature_offsets.push(sig_start as u32);
            msg_start_offsets.push(msg_start as u32);
            let msg_size = if msg_end < msg_start {
                0
            } else {
                msg_end - msg_start
            };
            msg_sizes.push(msg_size as u32);
            sig_lens.push(1);
            pubkeys_end += size_of::<Packet>();
        }
        v_sig_lens.push(sig_lens);
    }
    (signature_offsets, msg_start_offsets, msg_sizes, v_sig_lens)
}

pub fn verify_shreds_gpu(
    batches: &[Packets],
    slot_leaders: &HashMap<u64, [u8; 32]>,
    recycler_offsets: &Recycler<TxOffset>,
    recycler_out: &Recycler<PinnedVec<u8>>,
) -> Vec<Vec<u8>> {
    let api = perf_libs::api();
    if api.is_none() {
        return verify_shreds_cpu(batches, slot_leaders);
    }
    let api = api.unwrap();

    let mut elems = Vec::new();
    let mut rvs = Vec::new();
    let count = sigverify::batch_size(batches);
    let (pubkeys, pubkey_offsets, mut num_packets) =
        slot_key_data_for_gpu(0, batches, slot_leaders, recycler_offsets, recycler_out);
    //HACK: Pubkeys vector is passed along as a `Packets` buffer to the GPU
    //TODO: GPU needs a more opaque interface, which can handle variable sized structures for data
    let pubkeys_len = num_packets * size_of::<Packet>();
    trace!("num_packets: {}", num_packets);
    trace!("pubkeys_len: {}", pubkeys_len);
    let (signature_offsets, msg_start_offsets, msg_sizes, v_sig_lens) =
        shred_gpu_offsets(pubkeys_len, batches, recycler_offsets);
    let mut out = recycler_out.allocate("out_buffer");
    out.set_pinnable();
    elems.push(
        perf_libs::Elems {
            #![allow(clippy::cast_ptr_alignment)]
            elems: pubkeys.as_ptr() as *const solana_sdk::packet::Packet,
            num: num_packets as u32,
        },
    );

    for p in batches {
        elems.push(perf_libs::Elems {
            elems: p.packets.as_ptr(),
            num: p.packets.len() as u32,
        });
        let mut v = Vec::new();
        v.resize(p.packets.len(), 0);
        rvs.push(v);
        num_packets += p.packets.len();
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

    inc_new_counter_debug!("ed25519_shred_verify_gpu", count);
    recycler_out.recycle(out);
    recycler_out.recycle(pubkeys);
    recycler_offsets.recycle(signature_offsets);
    recycler_offsets.recycle(pubkey_offsets);
    recycler_offsets.recycle(msg_sizes);
    recycler_offsets.recycle(msg_start_offsets);
    rvs
}

/// Assuming layout is
/// signature: Signature
/// signed_msg: {
///   type: ShredType
///   slot: u64,
///   ...
/// }
/// Signature is the first thing in the packet, and slot is the first thing in the signed message.
fn sign_shred_cpu(
    packet: &mut Packet,
    slot_leaders_pubkeys: &HashMap<u64, [u8; 32]>,
    slot_leaders_privkeys: &HashMap<u64, [u8; 32]>,
) {
    let sig_start = 0;
    let sig_end = sig_start + size_of::<Signature>();
    let slot_start = sig_end + size_of::<ShredType>();
    let slot_end = slot_start + size_of::<u64>();
    let msg_start = sig_end;
    let msg_end = packet.meta.size;
    trace!("slot start and end {} {}", slot_start, slot_end);
    assert!(
        packet.meta.size >= slot_end,
        "packet is not large enough for a slot"
    );
    let slot: u64 =
        limited_deserialize(&packet.data[slot_start..slot_end]).expect("can't deserialize slot");
    trace!("slot {}", slot);
    let pubkey = slot_leaders_pubkeys
        .get(&slot)
        .expect("slot pubkey missing");
    let privkey = slot_leaders_privkeys
        .get(&slot)
        .expect("slot privkey missing");
    let keypair = Keypair {
        secret: SecretKey::from_bytes(&privkey[0..32]).expect("dalek privkey parser"),
        public: PublicKey::from_bytes(&pubkey[0..32]).expect("dalek pubkey parser"),
    };
    assert!(
        packet.meta.size >= sig_end,
        "packet is not large enough for a signature"
    );
    let signature = keypair.sign(&packet.data[msg_start..msg_end]);
    trace!("signature {:?}", signature);
    packet.data[0..sig_end].copy_from_slice(&signature.to_bytes());
}

fn sign_shreds_cpu(
    batches: &mut [Packets],
    slot_leaders_pubkeys: &HashMap<u64, [u8; 32]>,
    slot_leaders_privkeys: &HashMap<u64, [u8; 32]>,
) {
    use rayon::prelude::*;
    let count = sigverify::batch_size(batches);
    debug!("CPU SHRED ECDSA for {}", count);
    PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            batches.par_iter_mut().for_each(|p| {
                p.packets.iter_mut().for_each(|mut p| {
                    sign_shred_cpu(&mut p, slot_leaders_pubkeys, slot_leaders_privkeys)
                });
            });
        })
    });
    inc_new_counter_debug!("ed25519_shred_verify_cpu", count);
}

pub fn sign_shreds_gpu(
    batches: &mut [Packets],
    slot_leaders_pubkeys: &HashMap<u64, [u8; 32]>,
    slot_leaders_privkeys: &HashMap<u64, [u8; 32]>,
    recycler_offsets: &Recycler<TxOffset>,
    recycler_out: &Recycler<PinnedVec<u8>>,
) {
    let sig_size = size_of::<Signature>();
    let api = perf_libs::api();
    if api.is_none() {
        return sign_shreds_cpu(batches, slot_leaders_pubkeys, slot_leaders_privkeys);
    }
    let slot_leaders_secrets: HashMap<u64, Signature> = slot_leaders_privkeys
        .iter()
        .map(|(k, v)| {
            if *k == std::u64::MAX {
                (*k, Signature::default())
            } else {
                let mut hasher = Sha512::default();
                hasher.input(&v);
                let mut result = hasher.result();
                result[0] &= 248;
                result[31] &= 63;
                result[31] |= 64;
                let sig = Signature::new(result.as_slice());
                (*k, sig)
            }
        })
        .collect();
    let api = api.unwrap();

    let mut elems = Vec::new();
    let count = sigverify::batch_size(batches);
    let mut offset: usize = 0;
    let mut num_packets = 0;
    let (pubkeys, pubkey_offsets, num_pubkey_packets) = slot_key_data_for_gpu(
        offset,
        batches,
        slot_leaders_pubkeys,
        recycler_offsets,
        recycler_out,
    );
    offset += num_pubkey_packets * size_of::<Packet>();
    num_packets += num_pubkey_packets;
    trace!("offset: {}", offset);
    let (secrets, secret_offsets, num_secret_packets) = slot_key_data_for_gpu(
        offset,
        batches,
        &slot_leaders_secrets,
        recycler_offsets,
        recycler_out,
    );
    offset += num_secret_packets * size_of::<Packet>();
    num_packets += num_secret_packets;
    //HACK: Pubkeys vector is passed along as a `Packets` buffer to the GPU
    //TODO: GPU needs a more opaque interface, which can handle variable sized structures for data
    trace!("offset: {}", offset);
    let (signature_offsets, msg_start_offsets, msg_sizes, _v_sig_lens) =
        shred_gpu_offsets(offset, batches, recycler_offsets);
    let total_sigs = signature_offsets.len();
    let mut signatures_out = recycler_out.allocate("ed25519 signatures");
    signatures_out.resize(total_sigs * sig_size, 0);
    elems.push(
        perf_libs::Elems {
            #![allow(clippy::cast_ptr_alignment)]
            elems: pubkeys.as_ptr() as *const solana_sdk::packet::Packet,
            num: num_pubkey_packets as u32,
        },
    );

    elems.push(
        perf_libs::Elems {
            #![allow(clippy::cast_ptr_alignment)]
            elems: secrets.as_ptr() as *const solana_sdk::packet::Packet,
            num: num_secret_packets as u32,
        },
    );

    for p in batches.iter() {
        elems.push(perf_libs::Elems {
            elems: p.packets.as_ptr(),
            num: p.packets.len() as u32,
        });
        let mut v = Vec::new();
        v.resize(p.packets.len(), 0);
        num_packets += p.packets.len();
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
    sizes.extend(batches.iter().map(|b| b.packets.len()));
    PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            batches
                .par_iter_mut()
                .enumerate()
                .for_each(|(batch_ix, batch)| {
                    let num_packets = sizes[batch_ix];
                    batch
                        .packets
                        .iter_mut()
                        .enumerate()
                        .for_each(|(packet_ix, packet)| {
                            let sig_ix = packet_ix + num_packets;
                            let sig_start = sig_ix * sig_size;
                            let sig_end = sig_start + sig_size;
                            packet.data[0..sig_size]
                                .copy_from_slice(&signatures_out[sig_start..sig_end]);
                        });
                });
        });
    });
    inc_new_counter_debug!("ed25519_shred_sign_gpu", count);
    recycler_out.recycle(signatures_out);
    recycler_out.recycle(pubkeys);
    recycler_offsets.recycle(signature_offsets);
    recycler_offsets.recycle(pubkey_offsets);
    recycler_offsets.recycle(msg_sizes);
    recycler_offsets.recycle(msg_start_offsets);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::shred::{Shred, Shredder};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    #[test]
    fn test_sigverify_shred_cpu() {
        solana_logger::setup();
        let mut packet = Packet::default();
        let slot = 0xdeadc0de;
        let mut shred = Shred::new_from_data(slot, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        assert_eq!(shred.slot(), slot);
        let keypair = Keypair::new();
        Shredder::sign_shred(&keypair, &mut shred);
        trace!("signature {}", shred.common_header.signature);
        packet.data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        packet.meta.size = shred.payload.len();

        let leader_slots = [(slot, keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shred_cpu(&packet, &leader_slots);
        assert_eq!(rv, Some(1));

        let wrong_keypair = Keypair::new();
        let leader_slots = [(slot, wrong_keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shred_cpu(&packet, &leader_slots);
        assert_eq!(rv, Some(0));

        let leader_slots = HashMap::new();
        let rv = verify_shred_cpu(&packet, &leader_slots);
        assert_eq!(rv, None);
    }

    #[test]
    fn test_sigverify_shreds_cpu() {
        solana_logger::setup();
        let mut batch = [Packets::default()];
        let slot = 0xdeadc0de;
        let mut shred = Shred::new_from_data(slot, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        let keypair = Keypair::new();
        Shredder::sign_shred(&keypair, &mut shred);
        batch[0].packets.resize(1, Packet::default());
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();

        let leader_slots = [(slot, keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shreds_cpu(&batch, &leader_slots);
        assert_eq!(rv, vec![vec![1]]);

        let wrong_keypair = Keypair::new();
        let leader_slots = [(slot, wrong_keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shreds_cpu(&batch, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = HashMap::new();
        let rv = verify_shreds_cpu(&batch, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = [(slot, keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        batch[0].packets[0].meta.size = 0;
        let rv = verify_shreds_cpu(&batch, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);
    }

    #[test]
    fn test_sigverify_shreds_gpu() {
        solana_logger::setup();
        let recycler_offsets = Recycler::default();
        let recycler_out = Recycler::default();

        let mut batch = [Packets::default()];
        let slot = 0xdeadc0de;
        let mut shred = Shred::new_from_data(slot, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        let keypair = Keypair::new();
        Shredder::sign_shred(&keypair, &mut shred);
        batch[0].packets.resize(1, Packet::default());
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();

        let leader_slots = [
            (slot, keypair.pubkey().to_bytes()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(&batch, &leader_slots, &recycler_offsets, &recycler_out);
        assert_eq!(rv, vec![vec![1]]);

        let wrong_keypair = Keypair::new();
        let leader_slots = [
            (slot, wrong_keypair.pubkey().to_bytes()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(&batch, &leader_slots, &recycler_offsets, &recycler_out);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = [(std::u64::MAX, [0u8; 32])].iter().cloned().collect();
        let rv = verify_shreds_gpu(&batch, &leader_slots, &recycler_offsets, &recycler_out);
        assert_eq!(rv, vec![vec![0]]);

        batch[0].packets[0].meta.size = 0;
        let leader_slots = [
            (slot, keypair.pubkey().to_bytes()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(&batch, &leader_slots, &recycler_offsets, &recycler_out);
        assert_eq!(rv, vec![vec![0]]);
    }

    #[test]
    fn test_sigverify_shreds_sign_gpu() {
        solana_logger::setup();
        let recycler_offsets = Recycler::default();
        let recycler_out = Recycler::default();

        let mut batch = [Packets::default()];
        let slot = 0xdeadc0de;
        let keypair = Keypair::new();
        let shred = Shred::new_from_data(slot, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        batch[0].packets.resize(1, Packet::default());
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();
        let pubkeys = [
            (slot, keypair.pubkey().to_bytes()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let privkeys = [
            (slot, keypair.secret.to_bytes()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        //unsigned
        let rv = verify_shreds_gpu(&batch, &pubkeys, &recycler_offsets, &recycler_out);
        assert_eq!(rv, vec![vec![0]]);
        //signed
        sign_shreds_gpu(
            &mut batch,
            &pubkeys,
            &privkeys,
            &recycler_offsets,
            &recycler_out,
        );
        let rv = verify_shreds_cpu(&batch, &pubkeys);
        assert_eq!(rv, vec![vec![1]]);

        let rv = verify_shreds_gpu(&batch, &pubkeys, &recycler_offsets, &recycler_out);
        assert_eq!(rv, vec![vec![1]]);
    }

    #[test]
    fn test_sigverify_shreds_sign_cpu() {
        solana_logger::setup();

        let mut batch = [Packets::default()];
        let slot = 0xdeadc0de;
        let keypair = Keypair::new();
        let shred = Shred::new_from_data(slot, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        batch[0].packets.resize(1, Packet::default());
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();
        let pubkeys = [
            (slot, keypair.pubkey().to_bytes()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let privkeys = [
            (slot, keypair.secret.to_bytes()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        //unsigned
        let rv = verify_shreds_cpu(&batch, &pubkeys);
        assert_eq!(rv, vec![vec![0]]);
        //signed
        sign_shreds_cpu(&mut batch, &pubkeys, &privkeys);
        let rv = verify_shreds_cpu(&batch, &pubkeys);
        assert_eq!(rv, vec![vec![1]]);
    }
}
