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
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntries>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let mut receive_results = broadcast_utils::recv_slot_shreds(receiver)?;
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

        let (shreds, shred_infos, _) = broadcast_utils::entries_to_shreds(
            receive_results.ventries,
            bank.slot(),
            last_tick,
            bank.max_tick_height(),
            keypair,
            latest_blob_index,
            bank.parent().unwrap().slot(),
        );

        let seeds: Vec<[u8; 32]> = shreds.iter().map(|s| s.seed()).collect();

        blocktree.insert_shreds(shreds, None)?;

        // 3) Start broadcast step
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let shred_bufs: Vec<Vec<u8>> = shred_infos.into_iter().map(|s| s.shred).collect();
        // Broadcast data + erasures
        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            &shred_bufs,
            &seeds,
            stakes.as_ref(),
        )?;

        Ok(())
    }
}
