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
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    },
};

#[derive(Clone)]
pub struct ShredSigVerifier {
    pubkey: Pubkey, // TODO: Hot swap will change pubkey.
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    recycler_cache: RecyclerCache,
    retransmit_sender: Sender<Vec</*shred:*/ Vec<u8>>>,
    packet_sender: Sender<Vec<PacketBatch>>,
    turbine_disabled: Arc<AtomicBool>,
}

impl ShredSigVerifier {
    pub fn new(
        pubkey: Pubkey,
        bank_forks: Arc<RwLock<BankForks>>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
        retransmit_sender: Sender<Vec</*shred:*/ Vec<u8>>>,
        packet_sender: Sender<Vec<PacketBatch>>,
        turbine_disabled: Arc<AtomicBool>,
    ) -> Self {
        sigverify::init();
        Self {
            pubkey,
            bank_forks,
            leader_schedule_cache,
            recycler_cache: RecyclerCache::warmed(),
            retransmit_sender,
            packet_sender,
            turbine_disabled,
        }
    }
}

impl SigVerifier for ShredSigVerifier {
    type SendType = Vec<PacketBatch>;

    fn send_packets(
        &mut self,
        packet_batches: Vec<PacketBatch>,
    ) -> Result<(), SigVerifyServiceError<Self::SendType>> {
        if self.turbine_disabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Exclude repair packets from retransmit.
        // TODO: return the error here!
        let _ = self.retransmit_sender.send(
            packet_batches
                .iter()
                .flat_map(PacketBatch::iter)
                .filter(|packet| !packet.meta.discard() && !packet.meta.repair())
                .filter_map(shred::layout::get_shred)
                .map(<[u8]>::to_vec)
                .collect(),
        );
        self.packet_sender.send(packet_batches)?;
        Ok(())
    }

    fn verify_batches(
        &self,
        mut batches: Vec<PacketBatch>,
        _valid_packets: usize,
    ) -> Vec<PacketBatch> {
        let working_bank = self.bank_forks.read().unwrap().working_bank();
        let leader_slots: HashMap<Slot, [u8; 32]> = get_slot_leaders(
            &self.pubkey,
            &mut batches,
            &self.leader_schedule_cache,
            &working_bank,
        )
        .into_iter()
        .filter_map(|(slot, pubkey)| Some((slot, pubkey?.to_bytes())))
        .chain(std::iter::once((Slot::MAX, [0u8; 32])))
        .collect();
        let r = verify_shreds_gpu(&batches, &leader_slots, &self.recycler_cache);
        solana_perf::sigverify::mark_disabled(&mut batches, &r);
        batches
    }
}

// Returns pubkey of leaders for shred slots refrenced in the packets.
// Marks packets as discard if:
//   - fails to deserialize the shred slot.
//   - slot leader is unknown.
//   - slot leader is the node itself (circular transmission).
fn get_slot_leaders(
    self_pubkey: &Pubkey,
    batches: &mut [PacketBatch],
    leader_schedule_cache: &LeaderScheduleCache,
    bank: &Bank,
) -> HashMap<Slot, Option<Pubkey>> {
    let mut leaders = HashMap::<Slot, Option<Pubkey>>::new();
    for batch in batches {
        for packet in batch.iter_mut() {
            if packet.meta.discard() {
                continue;
            }
            let shred = shred::layout::get_shred(packet);
            let slot = match shred.and_then(shred::layout::get_slot) {
                None => {
                    packet.meta.set_discard(true);
                    continue;
                }
                Some(slot) => slot,
            };
            let leader = leaders.entry(slot).or_insert_with(|| {
                let leader = leader_schedule_cache.slot_leader_at(slot, Some(bank))?;
                // Discard the shred if the slot leader is the node itself.
                (&leader != self_pubkey).then(|| leader)
            });
            if leader.is_none() {
                packet.meta.set_discard(true);
            }
        }
    }
    leaders
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
    fn test_sigverify_shreds_verify_batches() {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let bank = Bank::new_for_tests(
            &create_genesis_config_with_leader(100, &leader_pubkey, 10).genesis_config,
        );
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let bf = Arc::new(RwLock::new(BankForks::new(bank)));
        let (sender, receiver) = unbounded();
        let (retransmit_sender, _retransmit_receiver) = unbounded();
        let mut verifier = ShredSigVerifier::new(
            Pubkey::new_unique(),
            bf,
            cache,
            retransmit_sender,
            sender,
            Arc::<AtomicBool>::default(), // turbine_disabled
        );
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
