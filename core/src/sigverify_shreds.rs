#![allow(clippy::implicit_hasher)]
use crate::packet::{limited_deserialize, Packets};
use crate::sigverify::{self, TxOffset};
use crate::sigverify_stage::SigVerifier;
use solana_ledger::bank_forks::BankForks;
use solana_ledger::leader_schedule_cache::LeaderScheduleCache;
use solana_ledger::shred::ShredType;
use solana_ledger::sigverify_shreds::verify_shreds_gpu;
use solana_perf::cuda_runtime::PinnedVec;
use solana_perf::recycler::Recycler;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct ShredSigVerifier {
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    recycler_offsets: Recycler<TxOffset>,
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
            recycler_offsets: Recycler::warmed(50, 4096),
            recycler_out: Recycler::warmed(50, 4096),
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
                        limited_deserialize(&packet.data[slot_start..slot_end]).ok()
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}

impl SigVerifier for ShredSigVerifier {
    fn verify_batch(&self, mut batches: Vec<Packets>) -> Vec<Packets> {
        let r_bank = self.bank_forks.read().unwrap().working_bank();
        let slots: HashSet<u64> = Self::read_slots(&batches);
        let mut leader_slots: HashMap<u64, [u8; 32]> = slots
            .into_iter()
            .filter_map(|slot| {
                let key = self
                    .leader_schedule_cache
                    .slot_leader_at(slot, Some(&r_bank))?;
                Some((slot, key.to_bytes()))
            })
            .collect();
        leader_slots.insert(std::u64::MAX, [0u8; 32]);

        let r = verify_shreds_gpu(
            &batches,
            &leader_slots,
            &self.recycler_offsets,
            &self.recycler_out,
        );
        sigverify::mark_disabled(&mut batches, &r);
        batches
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::genesis_utils::create_genesis_block_with_leader;
    use crate::packet::Packet;
    use solana_ledger::shred::{Shred, Shredder};
    use solana_runtime::bank::Bank;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_sigverify_shreds_read_slots() {
        solana_logger::setup();
        let mut shred = Shred::new_from_data(
            0xdeadc0de,
            0xc0de,
            0xdead,
            Some(&[1, 2, 3, 4]),
            true,
            true,
            0,
        );
        let mut batch = [Packets::default(), Packets::default()];

        let keypair = Keypair::new();
        Shredder::sign_shred(&keypair, &mut shred);
        batch[0].packets.resize(1, Packet::default());
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();

        let mut shred = Shred::new_from_data(
            0xc0dedead,
            0xc0de,
            0xdead,
            Some(&[1, 2, 3, 4]),
            true,
            true,
            0,
        );
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

        let mut shred = Shred::new_from_data(0, 0xc0de, 0xdead, Some(&[1, 2, 3, 4]), true, true, 0);
        Shredder::sign_shred(&leader_keypair, &mut shred);
        batch[0].packets[0].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[0].meta.size = shred.payload.len();

        let mut shred = Shred::new_from_data(0, 0xbeef, 0xc0de, Some(&[1, 2, 3, 4]), true, true, 0);
        let wrong_keypair = Keypair::new();
        Shredder::sign_shred(&wrong_keypair, &mut shred);
        batch[0].packets[1].data[0..shred.payload.len()].copy_from_slice(&shred.payload);
        batch[0].packets[1].meta.size = shred.payload.len();

        let rv = verifier.verify_batch(batch);
        assert_eq!(rv[0].packets[0].meta.discard, false);
        assert_eq!(rv[0].packets[1].meta.discard, true);
    }
}
