use super::*;
use solana_ledger::shred::{Shredder, RECOMMENDED_FEC_RATE};
use solana_sdk::hash::Hash;

pub(super) struct FailEntryVerificationBroadcastRun {}

impl FailEntryVerificationBroadcastRun {
    pub(super) fn new() -> Self {
        Self {}
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

        // 2) Convert entries to blobs + generate coding blobs. Set a garbage PoH on the last entry
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
        )
        .expect("Expected to create a new shredder");

        let (data_shreds, coding_shreds, _) = shredder.entries_to_shreds(
            &receive_results.entries,
            last_tick_height == bank.max_tick_height(),
            next_shred_index,
        );

        let all_shreds = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect::<Vec<_>>();
        let all_seeds: Vec<[u8; 32]> = all_shreds.iter().map(|s| s.seed()).collect();
        blocktree
            .insert_shreds(all_shreds, None, true)
            .expect("Failed to insert shreds in blocktree");

        // 3) Start broadcast step
        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let all_shred_bufs: Vec<Vec<u8>> = data_shreds
            .into_iter()
            .chain(coding_shreds.into_iter())
            .map(|s| s.payload)
            .collect();

        // Broadcast data
        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            all_shred_bufs,
            &all_seeds,
            stakes.as_ref(),
        )?;

        Ok(())
    }
}
