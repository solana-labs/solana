#![allow(clippy::implicit_hasher)]
use crate::cuda_runtime::PinnedVec;
use crate::packet::{Packet, Packets};
use crate::recycler::Recycler;
use crate::recycler::Reset;
use crate::sigverify::{self, TxOffset};
use crate::sigverify_stage::SigVerifier;
use crate::sigverify_stage::VerifiedPackets;
use bincode::deserialize;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::ThreadPool;
use solana_ed25519_dalek::{Keypair, PublicKey, SecretKey};
use solana_ledger::bank_forks::BankForks;
use solana_ledger::leader_schedule_cache::LeaderScheduleCache;
use solana_ledger::perf_libs;
use solana_ledger::shred::ShredType;
use solana_metrics::inc_new_counter_debug;
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::{Arc, RwLock};

use std::cell::RefCell;

#[derive(Clone)]
pub struct ShredSigVerifier {
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    recycler_offsets: Recycler<TxOffset>,
    recycler_pubkeys: Recycler<PinnedVec<[u8; 32]>>,
    recycler_out: Recycler<PinnedVec<u8>>,
}

impl ShredSigVerifier {
    pub fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
    ) -> Self {
        sigverify::init();
        Self {
            bank_forks,
            leader_schedule_cache,
            recycler_offsets: Recycler::default(),
            recycler_pubkeys: Recycler::default(),
            recycler_out: Recycler::default(),
        }
    }
    fn read_slots(batches: &[Packets]) -> HashSet<u64> {
        batches
            .iter()
            .flat_map(|batch| {
                batch.packets.iter().filter_map(|packet| {
                    let slot_start = size_of::<Signature>() + size_of::<ShredType>();
                    let slot_end = slot_start + size_of::<u64>();
                    trace!("slot {} {}", slot_start, slot_end,);
                    if slot_end <= packet.meta.size {
                        let slot: u64 = deserialize(&packet.data[slot_start..slot_end]).ok()?;
                        Some(slot)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}

impl SigVerifier for ShredSigVerifier {
    fn verify_batch(&self, batches: Vec<Packets>) -> VerifiedPackets {
        let r_bank = self.bank_forks.read().unwrap().working_bank();
        let slots: HashSet<u64> = Self::read_slots(&batches);
        let mut leader_slots: HashMap<u64, [u8; 32]> = slots
            .into_iter()
            .filter_map(|slot| {
                let key = self
                    .leader_schedule_cache
                    .slot_leader_at(slot, Some(&r_bank))?;
                Some((slot, key.to_array()))
            })
            .collect();
        leader_slots.insert(std::u64::MAX, [0u8; 32]);

        let r = verify_shreds_gpu(
            &batches,
            &leader_slots,
            &self.recycler_offsets,
            &self.recycler_pubkeys,
            &self.recycler_out,
        );
        batches.into_iter().zip(r).collect()
    }
}

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .thread_name(|ix| format!("sigverify_shreds_{}", ix))
                    .build()
                    .unwrap()));

impl Reset for PinnedVec<[u8; 32]> {
    fn reset(&mut self) {
        self.resize(0, [0u8; 32]);
    }
}

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
    let slot: u64 = deserialize(&packet.data[slot_start..slot_end]).ok()?;
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

fn shred_gpu_keys(
    offset_start: usize,
    batches: &[Packets],
    slot_leaders: &HashMap<u64, [u8; 32]>,
    recycler_offsets: &Recycler<TxOffset>,
    recycler_pubkeys: &Recycler<PinnedVec<[u8; 32]>>,
) -> (PinnedVec<[u8; 32]>, TxOffset, usize) {
    //TODO: mark Pubkey::default shreds as failed after the GPU returns
    assert_eq!(slot_leaders.get(&std::u64::MAX), Some(&[0u8; 32]));
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
                                deserialize(&packet.data[slot_start..slot_end]).ok();
                            match slot {
                                Some(slot) if slot_leaders.get(&slot).is_some() => slot,
                                _ => std::u64::MAX,
                            }
                        })
                        .collect()
                })
                .collect()
        })
    });
    let mut keys_to_slots: HashMap<[u8; 32], Vec<u64>> = HashMap::new();
    for batch in slots.iter() {
        for slot in batch.iter() {
            let key = slot_leaders.get(slot).unwrap();
            keys_to_slots
                .entry(*key)
                .or_insert_with(|| vec![])
                .push(*slot);
        }
    }
    let mut pubkeys: PinnedVec<[u8; 32]> = recycler_pubkeys.allocate("shred_gpu_pubkeys");
    let mut slot_to_key_ix = HashMap::new();
    for (i, (k, slots)) in keys_to_slots.iter().enumerate() {
        pubkeys.push(*k);
        for s in slots {
            slot_to_key_ix.insert(s, i);
        }
    }
    let mut offsets = recycler_offsets.allocate("shred_offsets");
    slots.iter().for_each(|packet_slots| {
        packet_slots.iter().for_each(|slot| {
            offsets.push(
                (offset_start + (slot_to_key_ix.get(slot).unwrap() * size_of::<[u8; 32]>())) as u32,
            );
        });
    });
    //HACK: Pubkeys vector is passed along as a `Packets` buffer to the GPU
    //TODO: GPU needs a more opaque interface, which can handle variable sized structures for data
    //Pad the Pubkeys buffer such that it is bigger than a buffer of Packet sized elems
    let num_in_packets =
        (pubkeys.len() * size_of::<[u8; 32]>() + (size_of::<Packet>() - 1)) / size_of::<Packet>();
    trace!("num_in_packets {}", num_in_packets);
    //number of bytes missing
    let missing = num_in_packets * size_of::<Packet>() - pubkeys.len() * size_of::<[u8; 32]>();
    trace!("missing {}", missing);
    //extra Pubkeys needed to fill the buffer
    let extra = (missing + size_of::<[u8; 32]>() - 1) / size_of::<[u8; 32]>();
    trace!("extra {}", extra);
    trace!("pubkeys {}", pubkeys.len());
    for _ in 0..extra {
        pubkeys.push([0u8; 32]);
        trace!("pubkeys {}", pubkeys.len());
    }
    trace!("pubkeys {:?}", pubkeys);
    trace!("offsets {:?}", offsets);
    (pubkeys, offsets, num_in_packets)
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

fn verify_shreds_gpu(
    batches: &[Packets],
    slot_leaders: &HashMap<u64, [u8; 32]>,
    recycler_offsets: &Recycler<TxOffset>,
    recycler_pubkeys: &Recycler<PinnedVec<[u8; 32]>>,
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
        shred_gpu_keys(0, batches, slot_leaders, recycler_offsets, recycler_pubkeys);
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
    recycler_offsets.recycle(signature_offsets);
    recycler_offsets.recycle(pubkey_offsets);
    recycler_offsets.recycle(msg_sizes);
    recycler_offsets.recycle(msg_start_offsets);
    recycler_pubkeys.recycle(pubkeys);
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
        deserialize(&packet.data[slot_start..slot_end]).expect("can't deserialize slot");
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
    let rv = PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            batches.par_iter_mut().for_each(|p| {
                p.packets.iter_mut().for_each(|mut p| {
                    sign_shred_cpu(&mut p, slot_leaders_pubkeys, slot_leaders_privkeys)
                });
            });
        })
    });
    inc_new_counter_debug!("ed25519_shred_verify_cpu", count);
    rv
}

pub fn sign_shreds_gpu(
    batches: &mut [Packets],
    slot_leaders_pubkeys: &HashMap<u64, [u8; 32]>,
    slot_leaders_privkeys: &HashMap<u64, [u8; 32]>,
    recycler_offsets: &Recycler<TxOffset>,
    recycler_pubkeys: &Recycler<PinnedVec<[u8; 32]>>,
) {
    let api = perf_libs::api();
    if api.is_none() {
        return sign_shreds_cpu(batches, slot_leaders_pubkeys, slot_leaders_privkeys);
    }
    let api = api.unwrap();

    let mut elems = Vec::new();
    let count = sigverify::batch_size(batches);
    let mut offset: usize = 0;
    let mut num_packets = 0;
    let (pubkeys, pubkey_offsets, num_pubkey_packets) = shred_gpu_keys(
        offset,
        batches,
        slot_leaders_pubkeys,
        recycler_offsets,
        recycler_pubkeys,
    );
    offset += num_pubkey_packets * size_of::<Packet>();
    num_packets += num_pubkey_packets;
    trace!("offset: {}", offset);
    let (privkeys, privkey_offsets, num_privkey_packets) = shred_gpu_keys(
        offset,
        batches,
        slot_leaders_privkeys,
        recycler_offsets,
        recycler_pubkeys,
    );
    offset += num_privkey_packets * size_of::<Packet>();
    num_packets += num_privkey_packets;
    //HACK: Pubkeys vector is passed along as a `Packets` buffer to the GPU
    //TODO: GPU needs a more opaque interface, which can handle variable sized structures for data
    trace!("offset: {}", offset);
    let (signature_offsets, msg_start_offsets, msg_sizes, _v_sig_lens) =
        shred_gpu_offsets(offset, batches, recycler_offsets);
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
            elems: privkeys.as_ptr() as *const solana_sdk::packet::Packet,
            num: num_privkey_packets as u32,
        },
    );

    for p in batches {
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
            signature_offsets.len() as u32,
            msg_sizes.as_ptr(),
            pubkey_offsets.as_ptr(),
            privkey_offsets.as_ptr(),
            signature_offsets.as_ptr(),
            msg_start_offsets.as_ptr(),
            USE_NON_DEFAULT_STREAM,
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done sign");

    inc_new_counter_debug!("ed25519_shred_sign_gpu", count);
    recycler_offsets.recycle(signature_offsets);
    recycler_offsets.recycle(pubkey_offsets);
    recycler_offsets.recycle(msg_sizes);
    recycler_offsets.recycle(msg_start_offsets);
    recycler_pubkeys.recycle(pubkeys);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::genesis_utils::create_genesis_block_with_leader;
    use solana_ledger::shred::{Shred, Shredder};
    use solana_runtime::bank::Bank;
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

        let leader_slots = [(slot, keypair.pubkey().to_array())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shred_cpu(&packet, &leader_slots);
        assert_eq!(rv, Some(1));

        let wrong_keypair = Keypair::new();
        let leader_slots = [(slot, wrong_keypair.pubkey().to_array())]
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

        let leader_slots = [(slot, keypair.pubkey().to_array())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shreds_cpu(&batch, &leader_slots);
        assert_eq!(rv, vec![vec![1]]);

        let wrong_keypair = Keypair::new();
        let leader_slots = [(slot, wrong_keypair.pubkey().to_array())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shreds_cpu(&batch, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = HashMap::new();
        let rv = verify_shreds_cpu(&batch, &leader_slots);
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = [(slot, keypair.pubkey().to_array())]
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
        let recycler_pubkeys = Recycler::default();
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
            (slot, keypair.pubkey().to_array()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(
            &batch,
            &leader_slots,
            &recycler_offsets,
            &recycler_pubkeys,
            &recycler_out,
        );
        assert_eq!(rv, vec![vec![1]]);

        let wrong_keypair = Keypair::new();
        let leader_slots = [
            (slot, wrong_keypair.pubkey().to_array()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(
            &batch,
            &leader_slots,
            &recycler_offsets,
            &recycler_pubkeys,
            &recycler_out,
        );
        assert_eq!(rv, vec![vec![0]]);

        let leader_slots = [(std::u64::MAX, [0u8; 32])].iter().cloned().collect();
        let rv = verify_shreds_gpu(
            &batch,
            &leader_slots,
            &recycler_offsets,
            &recycler_pubkeys,
            &recycler_out,
        );
        assert_eq!(rv, vec![vec![0]]);

        batch[0].packets[0].meta.size = 0;
        let leader_slots = [
            (slot, keypair.pubkey().to_array()),
            (std::u64::MAX, [0u8; 32]),
        ]
        .iter()
        .cloned()
        .collect();
        let rv = verify_shreds_gpu(
            &batch,
            &leader_slots,
            &recycler_offsets,
            &recycler_pubkeys,
            &recycler_out,
        );
        assert_eq!(rv, vec![vec![0]]);
    }

    #[test]
    fn test_sigverify_shreds_read_slots() {
        solana_logger::setup();
        let mut shred =
            Shred::new_from_data(0xdeadc0de, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        let mut batch = [Packets::default(), Packets::default()];

        let keypair = Keypair::new();
        Shredder::sign_shred(&keypair, &mut shred);
        batch[0].packets.resize(1, Packet::default());
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();

        let mut shred =
            Shred::new_from_data(0xc0dedead, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        Shredder::sign_shred(&keypair, &mut shred);
        batch[1].packets.resize(1, Packet::default());
        batch[1].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[1].packets[0].meta.size = shred.payload.len();

        let expected: HashSet<u64> = [0xc0dedead, 0xdeadc0de].iter().cloned().collect();
        assert_eq!(ShredSigVerifier::read_slots(&batch), expected);
    }

    #[test]
    fn test_sigverify_shreds_verify_batch() {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let bank =
            Bank::new(&create_genesis_block_with_leader(100, &leader_pubkey, 10).genesis_block);
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let bf = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let verifier = ShredSigVerifier::new(bf, cache);

        let mut batch = vec![Packets::default()];
        batch[0].packets.resize(2, Packet::default());

        let mut shred = Shred::new_from_data(0, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true);
        Shredder::sign_shred(&leader_keypair, &mut shred);
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();

        let mut shred = Shred::new_from_data(0, 0xbeef, 0xc0de, Some(&[1, 2, 3, 4]), true, true);
        let wrong_keypair = Keypair::new();
        Shredder::sign_shred(&wrong_keypair, &mut shred);
        batch[0].packets[1].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[1].meta.size = shred.payload.len();

        let rv = verifier.verify_batch(batch);
        assert_eq!(rv[0].1, vec![1, 0]);
    }
}
