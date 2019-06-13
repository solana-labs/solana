use super::*;
use crate::entry::EntrySlice;
use rayon::prelude::*;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signable;

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

        let blobs: Vec<_> = broadcast.thread_pool.install(|| {
            receive_results
                .ventries
                .into_par_iter()
                .map(|p| {
                    let entries: Vec<_> = p.into_iter().map(|e| e.0).collect();
                    entries.to_shared_blobs()
                })
                .flatten()
                .collect()
        });

        let blob_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0);

        index_blobs(
            &blobs,
            &broadcast.id,
            blob_index,
            bank.slot(),
            bank.parent().map_or(0, |parent| parent.slot()),
        );

        if last_tick == bank.max_tick_height() {
            blobs.last().unwrap().write().unwrap().set_is_last_in_slot();
        }

        // Make sure not to modify the blob header or data after signing it here
        broadcast.thread_pool.install(|| {
            blobs.par_iter().for_each(|b| {
                b.write()
                    .unwrap()
                    .sign(&cluster_info.read().unwrap().keypair);
            })
        });

        blocktree.write_shared_blobs(&blobs)?;

        let coding = broadcast.coding_generator.next(&blobs);

        broadcast.thread_pool.install(|| {
            coding.par_iter().for_each(|c| {
                c.write()
                    .unwrap()
                    .sign(&cluster_info.read().unwrap().keypair);
            })
        });

        // 3) Start broadcast step
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        // Broadcast data
        cluster_info
            .read()
            .unwrap()
            .broadcast(sock, &blobs, stakes.as_ref())?;

        // Broadcast erasures
        cluster_info
            .read()
            .unwrap()
            .broadcast(sock, &coding, stakes.as_ref())?;

        Ok(())
    }
}
