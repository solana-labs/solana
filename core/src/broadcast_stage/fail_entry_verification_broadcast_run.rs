use super::*;
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
        broadcast: &mut Broadcast,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntries>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let mut receive_results = broadcast_utils::recv_slot_blobs(receiver)?;
        let bank = receive_results.bank.clone();
        let last_tick = receive_results.last_tick;

        // 2) Convert entries to blobs + generate coding blobs. Set a garbage PoH on the last entry
        // in the slot to make verification fail on validators
        if last_tick == bank.max_tick_height() {
            let mut last_entry = receive_results
                .ventries
                .last_mut()
                .unwrap()
                .last_mut()
                .unwrap();
            last_entry.0.hash = Hash::default();
        }

        let keypair = &cluster_info.read().unwrap().keypair.clone();
        let latest_blob_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0);

        let (data_blobs, coding_blobs) = broadcast_utils::entries_to_blobs(
            receive_results.ventries,
            &broadcast.thread_pool,
            latest_blob_index,
            last_tick,
            &bank,
            &keypair,
            &mut broadcast.coding_generator,
        );

        blocktree.write_shared_blobs(data_blobs.iter())?;
        blocktree.put_shared_coding_blobs(coding_blobs.iter())?;

        // 3) Start broadcast step
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        // Broadcast data + erasures
        cluster_info.read().unwrap().broadcast(
            sock,
            data_blobs.iter().chain(coding_blobs.iter()),
            stakes.as_ref(),
        )?;

        Ok(())
    }
}
