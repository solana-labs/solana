use super::broadcast_utils;
use super::*;
use crate::broadcast_stage::broadcast_utils::entries_to_shreds;
use solana_sdk::timing::duration_as_ms;

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
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntry>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        let receive_elapsed = receive_results.time_elapsed;
        let num_entries = receive_results.entries.len();
        let bank = receive_results.bank.clone();
        let last_tick = receive_results.last_tick;
        inc_new_counter_info!("broadcast_service-entries_received", num_entries);

        // 2) Convert entries to blobs + generate coding blobs
        let to_blobs_start = Instant::now();
        let keypair = &cluster_info.read().unwrap().keypair.clone();
        let latest_shred_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0);

        let parent_slot = if let Some(parent_bank) = bank.parent() {
            parent_bank.slot()
        } else {
            0
        };

        let (all_shreds, shred_infos, latest_shred_index) = entries_to_shreds(
            receive_results.entries,
            last_tick,
            bank.slot(),
            bank.max_tick_height(),
            keypair,
            latest_shred_index,
            parent_slot,
        );

        let all_seeds: Vec<[u8; 32]> = shred_infos.iter().map(|s| s.seed()).collect();
        let num_shreds = all_shreds.len();
        blocktree
            .insert_shreds(shred_infos.clone(), None)
            .expect("Failed to insert shreds in blocktree");

        let to_blobs_elapsed = to_blobs_start.elapsed();

        // 3) Start broadcast step
        let broadcast_start = Instant::now();
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let all_shred_bufs: Vec<Vec<u8>> = shred_infos.into_iter().map(|s| s.shred).collect();
        trace!("Broadcasting {:?} shreds", all_shred_bufs.len());
        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            &all_shred_bufs,
            &all_seeds,
            stakes.as_ref(),
        )?;

        inc_new_counter_debug!("streamer-broadcast-sent", num_shreds);

        let broadcast_elapsed = broadcast_start.elapsed();
        self.update_broadcast_stats(
            duration_as_ms(&broadcast_elapsed),
            duration_as_ms(&(receive_elapsed + to_blobs_elapsed + broadcast_elapsed)),
            num_entries,
            duration_as_ms(&to_blobs_elapsed),
            latest_shred_index,
        );

        Ok(())
    }
}
