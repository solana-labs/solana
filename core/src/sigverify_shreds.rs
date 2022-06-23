#![allow(clippy::implicit_hasher)]

use {
    crate::{
        sigverify,
        sigverify_stage::{SigVerifier, SigVerifyServiceError},
    },
    crossbeam_channel::Sender,
    solana_ledger::{
        leader_schedule_cache::LeaderScheduleCache, shred, sigverify_shreds::verify_shreds_gpu,
    },
    solana_perf::{self, packet::PacketBatch, recycler_cache::RecyclerCache},
    solana_runtime::bank_forks::BankForks,
    solana_sdk::clock::Slot,
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
    packet_sender: Sender<Vec<PacketBatch>>,
}

impl ShredSigVerifier {
    pub fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
        packet_sender: Sender<Vec<PacketBatch>>,
    ) -> Self {
        sigverify::init();
        Self {
            bank_forks,
            leader_schedule_cache,
            recycler_cache: RecyclerCache::warmed(),
            packet_sender,
        }
    }
    fn read_slots(batches: &[PacketBatch]) -> HashSet<Slot> {
        batches
            .iter()
            .flat_map(PacketBatch::iter)
            .filter(|packet| !packet.meta.discard())
            .filter_map(shred::layout::get_shred)
            .filter_map(shred::layout::get_slot)
            .collect()
    }
}

impl SigVerifier for ShredSigVerifier {
    type SendType = Vec<PacketBatch>;

    fn send_packets(
        &mut self,
        packet_batches: Vec<PacketBatch>,
    ) -> Result<(), SigVerifyServiceError<Self::SendType>> {
        self.packet_sender.send(packet_batches)?;
        Ok(())
    }

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
        crossbeam_channel::unbounded,
        solana_ledger::{
            genesis_utils::create_genesis_config_with_leader,
            shred::{Shred, ShredFlags},
        },
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
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        let mut batches: Vec<_> = (0..2)
            .map(|_| {
                let mut batch = PacketBatch::with_capacity(1);
                batch.resize(1, Packet::default());
                batch
            })
            .collect();

        let keypair = Keypair::new();
        shred.sign(&keypair);
        batches[0][0].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][0].meta.size = shred.payload().len();

        let mut shred = Shred::new_from_data(
            0xc0de_dead,
            0xc0de,
            0xdead,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        shred.sign(&keypair);
        batches[1][0].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[1][0].meta.size = shred.payload().len();

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
        let (sender, receiver) = unbounded();
        let mut verifier = ShredSigVerifier::new(bf, cache, sender);

        let batch_size = 2;
        let mut batch = PacketBatch::with_capacity(batch_size);
        batch.resize(batch_size, Packet::default());
        let mut batches = vec![batch];

        let mut shred = Shred::new_from_data(
            0,
            0xc0de,
            0xdead,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        shred.sign(&leader_keypair);
        batches[0][0].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][0].meta.size = shred.payload().len();

        let mut shred = Shred::new_from_data(
            0,
            0xbeef,
            0xc0de,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        let wrong_keypair = Keypair::new();
        shred.sign(&wrong_keypair);
        batches[0][1].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][1].meta.size = shred.payload().len();

        let num_packets = solana_perf::sigverify::count_packets_in_batches(&batches);
        let rv = verifier.verify_batches(batches, num_packets);
        assert!(!rv[0][0].meta.discard());
        assert!(rv[0][1].meta.discard());

        verifier.send_packets(rv.clone()).unwrap();
        let received_packets = receiver.recv().unwrap();
        assert_eq!(received_packets.len(), rv.len());
        for (received_packet_batch, original_packet_batch) in received_packets.iter().zip(rv.iter())
        {
            assert_eq!(
                received_packet_batch.iter().collect::<Vec<_>>(),
                original_packet_batch.iter().collect::<Vec<_>>()
            );
        }
    }
}
