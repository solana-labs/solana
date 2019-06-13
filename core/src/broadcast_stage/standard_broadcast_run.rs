use super::broadcast_utils;
use super::*;
use crate::entry::EntrySlice;
use rayon::prelude::*;
use solana_sdk::signature::Signable;

#[derive(Default)]
struct BroadcastStats {
    num_entries: Vec<usize>,
    run_elapsed: Vec<u64>,
    to_blobs_elapsed: Vec<u64>,
}

pub(super) struct StandardBroadcastRun {
    stats: BroadcastStats,
}

impl StandardBroadcastRun {
    pub(super) fn new() -> Self {
        Self {
            stats: BroadcastStats::default(),
        }
    }

    fn update_broadcast_stats(
        &mut self,
        broadcast_elapsed: u64,
        run_elapsed: u64,
        num_entries: usize,
        to_blobs_elapsed: u64,
        blob_index: u64,
    ) {
        inc_new_counter_info!("broadcast_service-time_ms", broadcast_elapsed as usize);

        self.stats.num_entries.push(num_entries);
        self.stats.to_blobs_elapsed.push(to_blobs_elapsed);
        self.stats.run_elapsed.push(run_elapsed);
        if self.stats.num_entries.len() >= 16 {
            info!(
                "broadcast: entries: {:?} blob times ms: {:?} broadcast times ms: {:?}",
                self.stats.num_entries, self.stats.to_blobs_elapsed, self.stats.run_elapsed
            );
            self.stats.num_entries.clear();
            self.stats.to_blobs_elapsed.clear();
            self.stats.run_elapsed.clear();
        }

        datapoint!("broadcast-service", ("transmit-index", blob_index, i64));
    }
}

impl BroadcastRun for StandardBroadcastRun {
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
        let receive_elapsed = receive_results.time_elapsed;
        let num_entries = receive_results.num_entries;
        let bank = receive_results.bank.clone();
        let last_tick = receive_results.last_tick;
        inc_new_counter_info!("broadcast_service-entries_received", num_entries);

        // 2) Convert entries to blobs + generate coding blobs
        let to_blobs_start = Instant::now();
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

        let to_blobs_elapsed = to_blobs_start.elapsed();

        // 3) Start broadcast step
        let broadcast_start = Instant::now();
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        // Broadcast data
        cluster_info
            .read()
            .unwrap()
            .broadcast(sock, &blobs, stakes.as_ref())?;

        inc_new_counter_debug!("streamer-broadcast-sent", blobs.len());

        // Broadcast erasures
        cluster_info
            .read()
            .unwrap()
            .broadcast(sock, &coding, stakes.as_ref())?;

        let broadcast_elapsed = broadcast_start.elapsed();
        self.update_broadcast_stats(
            duration_as_ms(&broadcast_elapsed),
            duration_as_ms(&(receive_elapsed + to_blobs_elapsed + broadcast_elapsed)),
            num_entries,
            duration_as_ms(&to_blobs_elapsed),
            blob_index,
        );

        Ok(())
    }
}
