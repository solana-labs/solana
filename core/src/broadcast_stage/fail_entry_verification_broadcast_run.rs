use super::*;
use solana_ledger::shred::{ShredCommonHeader, Shredder, RECOMMENDED_FEC_RATE};
use solana_perf::packet::Packets;
use solana_perf::recycler_cache::RecyclerCache;
use solana_sdk::hash::Hash;

pub(super) struct FailEntryVerificationBroadcastRun {
    shred_version: u16,
    recycler_cache: RecyclerCache,
}

impl FailEntryVerificationBroadcastRun {
    pub(super) fn new(shred_version: u16) -> Self {
        Self {
            shred_version,
            recycler_cache: RecyclerCache::warmed(),
        }
    }
}

impl BroadcastRun for FailEntryVerificationBroadcastRun {
    fn run(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntry>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let mut receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        let bank = receive_results.bank.clone();
        let last_tick_height = receive_results.last_tick_height;

        // 2) Convert entries to shreds + generate coding shreds. Set a garbage PoH on the last entry
        // in the slot to make verification fail on validators
        if last_tick_height == bank.max_tick_height() {
            let mut last_entry = receive_results.entries.last_mut().unwrap();
            last_entry.hash = Hash::default();
        }

        let keypair = cluster_info.read().unwrap().keypair.clone();
        let next_shred_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0) as u32;

        let shredder = Shredder::new(
            bank.slot(),
            bank.parent().unwrap().slot(),
            RECOMMENDED_FEC_RATE,
            keypair.clone(),
            (bank.tick_height() % bank.ticks_per_slot()) as u8,
            self.shred_version,
        )
        .expect("Expected to create a new shredder");

        let (data_shreds, coding_shreds, _) = shredder.entries_to_shreds(
            &self.recycler_cache,
            &receive_results.entries,
            last_tick_height == bank.max_tick_height(),
            next_shred_index,
        );

        let mut all_shreds: Vec<Packets> = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect();
        let all_seeds: Vec<Vec<[u8; 32]>> = all_shreds
            .iter()
            .map(|p| {
                p.packets
                    .iter()
                    .map(|p| {
                        ShredCommonHeader::from_packet(p)
                            .expect("invalid packet")
                            .seed()
                    })
                    .collect()
            })
            .collect();
        blocktree
            .insert_batch(&all_shreds, None, true)
            .expect("Failed to insert shreds in blocktree");

        // 3) Start broadcast step
        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            &mut all_shreds,
            &all_seeds,
            stakes.as_ref(),
        )?;

        Ok(())
    }
}
