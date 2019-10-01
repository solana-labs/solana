use super::broadcast_utils;
use super::*;
use crate::broadcast_stage::broadcast_utils::{entries_to_shreds, UnfinishedSlotInfo};
use solana_sdk::timing::duration_as_ms;

#[derive(Default)]
struct BroadcastStats {
    num_entries: Vec<usize>,
    run_elapsed: Vec<u64>,
    to_blobs_elapsed: Vec<u64>,
}

pub(super) struct StandardBroadcastRun {
    stats: BroadcastStats,
    unfinished_slot: Option<UnfinishedSlotInfo>,
}

impl StandardBroadcastRun {
    pub(super) fn new() -> Self {
        Self {
            stats: BroadcastStats::default(),
            unfinished_slot: None,
        }
    }

    fn update_broadcast_stats(
        &mut self,
        receive_entries_elapsed: u64,
        shredding_elapsed: u64,
        insert_shreds_elapsed: u64,
        broadcast_elapsed: u64,
        run_elapsed: u64,
        num_entries: usize,
        num_shreds: usize,
        blob_index: u64,
    ) {
        inc_new_counter_info!("broadcast_service-time_ms", broadcast_elapsed as usize);

        self.stats.num_entries.push(num_entries);
        self.stats.to_blobs_elapsed.push(shredding_elapsed);
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

        datapoint_info!(
            "broadcast-service",
            ("num_entries", num_entries as i64, i64),
            ("num_shreds", num_shreds as i64, i64),
            ("receive_time", receive_entries_elapsed as i64, i64),
            ("shredding_time", shredding_elapsed as i64, i64),
            ("insert_shred_time", insert_shreds_elapsed as i64, i64),
            ("broadcast_time", broadcast_elapsed as i64, i64),
            ("transmit-index", blob_index as i64, i64),
        );
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

        let to_shreds_start = Instant::now();
        let (shred_infos, uninished_slot) = entries_to_shreds(
            receive_results.entries,
            last_tick,
            bank.slot(),
            bank.max_tick_height(),
            keypair,
            latest_shred_index,
            parent_slot,
            self.unfinished_slot,
        );
        let to_shreds_elapsed = to_shreds_start.elapsed();
        self.unfinished_slot = uninished_slot;

        let all_seeds: Vec<[u8; 32]> = shred_infos.iter().map(|s| s.seed()).collect();
        let num_shreds = shred_infos.len();
        let insert_shreds_start = Instant::now();
        blocktree
            .insert_shreds(shred_infos.clone(), None)
            .expect("Failed to insert shreds in blocktree");
        let insert_shreds_elapsed = insert_shreds_start.elapsed();

        // 3) Start broadcast step
        let broadcast_start = Instant::now();
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let all_shred_bufs: Vec<Vec<u8>> = shred_infos.into_iter().map(|s| s.payload).collect();
        trace!("Broadcasting {:?} shreds", all_shred_bufs.len());
        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            &all_shred_bufs,
            &all_seeds,
            stakes.as_ref(),
        )?;

        let broadcast_elapsed = broadcast_start.elapsed();
        let latest_shred_index = uninished_slot.map(|s| s.next_index).unwrap_or_else(|| {
            blocktree
                .meta(bank.slot())
                .expect("Database error")
                .map(|meta| meta.consumed)
                .unwrap_or(0)
        });
        self.update_broadcast_stats(
            duration_as_ms(&receive_elapsed),
            duration_as_ms(&to_shreds_elapsed),
            duration_as_ms(&insert_shreds_elapsed),
            duration_as_ms(&broadcast_elapsed),
            duration_as_ms(
                &(receive_elapsed + to_shreds_elapsed + insert_shreds_elapsed + broadcast_elapsed),
            ),
            num_entries,
            num_shreds,
            latest_shred_index,
        );

        Ok(())
    }
}
