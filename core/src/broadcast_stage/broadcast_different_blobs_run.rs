use super::*;
use crate::entry::Entry;
use solana_sdk::hash::Hash;

pub(super) struct BroadcastDifferentBlobsRun {
    last_blockhash: Hash,
}

impl BroadcastDifferentBlobsRun {
    pub(super) fn new() -> Self {
        Self {
            last_blockhash: Hash::default(),
        }
    }
}

impl BroadcastRun for BroadcastDifferentBlobsRun {
    fn run(
        &mut self,
        broadcast: &mut Broadcast,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntries>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let receive_results = broadcast_utils::recv_slot_blobs(receiver)?;
        let bank = receive_results.bank.clone();
        let last_tick = receive_results.last_tick;

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

        // If the last blockhash is default, a new block is being created
        // So grab the last blockhash from the parent bank
        if self.last_blockhash == Hash::default() {
            self.last_blockhash = bank.parent().unwrap().last_blockhash();
        }

        let fake_ventries: Vec<_> = (0..receive_results.num_entries)
            .map(|_| vec![(Entry::new(&self.last_blockhash, 0, vec![]), 0)])
            .collect();

        let (fake_data_blobs, fake_coding_blobs) = broadcast_utils::entries_to_blobs(
            fake_ventries,
            &broadcast.thread_pool,
            latest_blob_index,
            last_tick,
            &bank,
            &keypair,
            &mut broadcast.coding_generator,
        );

        // If it's the last tick, reset the last block hash to default
        // this will cause next run to grab last bank's blockhash
        if last_tick == bank.max_tick_height() {
            self.last_blockhash = Hash::default();
        }

        blocktree.write_shared_blobs(data_blobs.iter().chain(coding_blobs.iter()))?;

        // 3) Start broadcast step
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        // Broadcast data + erasures
        cluster_info.read().unwrap().broadcast(
            sock,
            data_blobs.iter().chain(coding_blobs.iter()),
            stakes.as_ref(),
        )?;

        // Broadcast fake data + erasures
        cluster_info.read().unwrap().broadcast(
            sock,
            fake_data_blobs.iter().chain(fake_coding_blobs.iter()),
            stakes.as_ref(),
        )?;

        Ok(())
    }
}
