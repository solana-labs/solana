#![allow(clippy::implicit_hasher)]
use {
    crate::{sigverify, sigverify_stage::SigVerifier},
    solana_ledger::{
        leader_schedule_cache::LeaderScheduleCache, shred::Shred,
        sigverify_shreds::verify_shreds_gpu,
    },
    solana_perf::{self, packet::PacketBatch, recycler_cache::RecyclerCache},
    solana_runtime::bank_forks::BankForks,
    std::{
        collections::{HashMap, HashSet},
        sync::{Arc, RwLock},
    },
};

#[derive(Clone)]
pub struct ShredSigVerifier {
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    recycler_cache: RecyclerCache,
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
            recycler_cache: RecyclerCache::warmed(),
        }
    }
    fn read_slots(batches: &[PacketBatch]) -> HashSet<u64> {
        batches
            .iter()
            .flat_map(|batch| batch.packets.iter().filter_map(Shred::get_slot_from_packet))
            .collect()
    }
}

impl SigVerifier for ShredSigVerifier {
    fn verify_batches(
        &self,
        mut batches: Vec<PacketBatch>,
        _valid_packets: usize,
    ) -> Vec<PacketBatch> {
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

        let r = verify_shreds_gpu(&batches, &leader_slots, &self.recycler_cache);
        solana_perf::sigverify::mark_disabled(&mut batches, &r);
        batches
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        solana_ledger::{genesis_utils::create_genesis_config_with_leader, shred::Shred},
        solana_perf::packet::Packet,
        solana_runtime::bank::Bank,
        solana_sdk::signature::{Keypair, Signer},
    };

    #[test]
    fn test_sigverify_shreds_read_slots() {
        solana_logger::setup();
        let mut shred = Shred::new_from_data(
            0xdead_c0de,
            0xc0de,
            0xdead,
            Some(&[1, 2, 3, 4]),
            true,
            true,
            0,
            0,
            0xc0de,
        );
        let mut batches = [PacketBatch::default(), PacketBatch::default()];

        let keypair = Keypair::new();
        shred.sign(&keypair);
        batches[0].packets.resize(1, Packet::default());
        batches[0].packets[0].data[0..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0].packets[0].meta.size = shred.payload().len();

        let mut shred = Shred::new_from_data(
            0xc0de_dead,
            0xc0de,
            0xdead,
            Some(&[1, 2, 3, 4]),
            true,
            true,
            0,
            0,
            0xc0de,
        );
        shred.sign(&keypair);
        batches[1].packets.resize(1, Packet::default());
        batches[1].packets[0].data[0..shred.payload().len()].copy_from_slice(shred.payload());
        batches[1].packets[0].meta.size = shred.payload().len();

        let expected: HashSet<u64> = [0xc0de_dead, 0xdead_c0de].iter().cloned().collect();
        assert_eq!(ShredSigVerifier::read_slots(&batches), expected);
    }

    #[test]
    fn test_sigverify_shreds_verify_batches() {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let bank = Bank::new_for_tests(
            &create_genesis_config_with_leader(100, &leader_pubkey, 10).genesis_config,
        );
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let bf = Arc::new(RwLock::new(BankForks::new(bank)));
        let verifier = ShredSigVerifier::new(bf, cache);

        let mut batches = vec![PacketBatch::default()];
        batches[0].packets.resize(2, Packet::default());

        let mut shred = Shred::new_from_data(
            0,
            0xc0de,
            0xdead,
            Some(&[1, 2, 3, 4]),
            true,
            true,
            0,
            0,
            0xc0de,
        );
        shred.sign(&leader_keypair);
        batches[0].packets[0].data[0..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0].packets[0].meta.size = shred.payload().len();

        let mut shred = Shred::new_from_data(
            0,
            0xbeef,
            0xc0de,
            Some(&[1, 2, 3, 4]),
            true,
            true,
            0,
            0,
            0xc0de,
        );
        let wrong_keypair = Keypair::new();
        shred.sign(&wrong_keypair);
        batches[0].packets[1].data[0..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0].packets[1].meta.size = shred.payload().len();

        let num_packets = solana_perf::sigverify::count_packets_in_batches(&batches);
        let rv = verifier.verify_batches(batches, num_packets);
        assert!(!rv[0].packets[0].meta.discard());
        assert!(rv[0].packets[1].meta.discard());
    }
}
