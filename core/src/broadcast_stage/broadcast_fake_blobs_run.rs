use super::*;
use crate::entry::Entry;
use solana_sdk::hash::Hash;

pub(super) struct BroadcastFakeBlobsRun {
    last_blockhash: Hash,
    partition: usize,
}

impl BroadcastFakeBlobsRun {
    pub(super) fn new(partition: usize) -> Self {
        Self {
            last_blockhash: Hash::default(),
            partition,
        }
    }
}

impl BroadcastRun for BroadcastFakeBlobsRun {
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

        // Set the forwarded flag to true, so that the blobs won't be forwarded to peers
        data_blobs
            .iter()
            .for_each(|blob| blob.write().unwrap().set_forwarded(true));
        coding_blobs
            .iter()
            .for_each(|blob| blob.write().unwrap().set_forwarded(true));
        fake_data_blobs
            .iter()
            .for_each(|blob| blob.write().unwrap().set_forwarded(true));
        fake_coding_blobs
            .iter()
            .for_each(|blob| blob.write().unwrap().set_forwarded(true));

        // 3) Start broadcast step
        let peers = cluster_info.read().unwrap().tvu_peers();
        peers.iter().enumerate().for_each(|(i, peer)| {
            if i <= self.partition {
                // Send fake blobs to the first N peers
                fake_data_blobs.iter().for_each(|b| {
                    let blob = b.read().unwrap();
                    sock.send_to(&blob.data[..blob.meta.size], &peer.tvu)
                        .unwrap();
                });
                fake_coding_blobs.iter().for_each(|b| {
                    let blob = b.read().unwrap();
                    sock.send_to(&blob.data[..blob.meta.size], &peer.tvu)
                        .unwrap();
                });
            } else {
                data_blobs.iter().for_each(|b| {
                    let blob = b.read().unwrap();
                    sock.send_to(&blob.data[..blob.meta.size], &peer.tvu)
                        .unwrap();
                });
                coding_blobs.iter().for_each(|b| {
                    let blob = b.read().unwrap();
                    sock.send_to(&blob.data[..blob.meta.size], &peer.tvu)
                        .unwrap();
                });
            }
        });

        Ok(())
    }
}
