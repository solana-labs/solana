use super::broadcast_utils;
use super::*;
use crate::shred::{Shredder, RECOMMENDED_FEC_RATE};
use solana_sdk::timing::duration_as_ms;

#[derive(Default)]
struct BroadcastStats {
    num_entries: Vec<usize>,
    run_elapsed: Vec<u64>,
    to_blobs_elapsed: Vec<u64>,
}

pub(super) struct StandardBroadcastRun {
    stats: BroadcastStats,
    current_slot: Option<u64>,
    shredding_elapsed: u128,
    insertion_elapsed: u128,
    broadcast_elapsed: u128,
    slot_broadcast_start: Option<Instant>,
}

impl StandardBroadcastRun {
    pub(super) fn new() -> Self {
        Self {
            stats: BroadcastStats::default(),
            current_slot: None,
            shredding_elapsed: 0,
            insertion_elapsed: 0,
            broadcast_elapsed: 0,
            slot_broadcast_start: None,
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
        shred_index: u32,
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

        datapoint_debug!(
            "broadcast-service",
            ("num_entries", num_entries as i64, i64),
            ("num_shreds", num_shreds as i64, i64),
            ("receive_time", receive_entries_elapsed as i64, i64),
            ("shredding_time", shredding_elapsed as i64, i64),
            ("insert_shred_time", insert_shreds_elapsed as i64, i64),
            ("broadcast_time", broadcast_elapsed as i64, i64),
            ("transmit-index", i64::from(shred_index), i64),
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

        if Some(bank.slot()) != self.current_slot {
            self.slot_broadcast_start = Some(Instant::now());
            self.current_slot = Some(bank.slot());
        }

        // 2) Convert entries to blobs + generate coding blobs
        let keypair = &cluster_info.read().unwrap().keypair.clone();
        let next_shred_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0) as u32;

        let parent_slot = if let Some(parent_bank) = bank.parent() {
            parent_bank.slot()
        } else {
            0
        };

        // Create shreds from entries
        let to_shreds_start = Instant::now();
        let shredder = Shredder::new(
            bank.slot(),
            parent_slot,
            RECOMMENDED_FEC_RATE,
            keypair.clone(),
        )
        .expect("Expected to create a new shredder");

        let (data_shreds, coding_shreds, latest_shred_index) = shredder.entries_to_shreds(
            &receive_results.entries,
            last_tick == bank.max_tick_height(),
            next_shred_index,
        );
        let to_shreds_elapsed = to_shreds_start.elapsed();

        let all_shreds = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect::<Vec<_>>();
        let all_seeds: Vec<[u8; 32]> = all_shreds.iter().map(|s| s.seed()).collect();
        let num_shreds = all_shreds.len();

        // Insert shreds into blocktree
        let insert_shreds_start = Instant::now();
        blocktree
            .insert_shreds(all_shreds, None)
            .expect("Failed to insert shreds in blocktree");
        let insert_shreds_elapsed = insert_shreds_start.elapsed();

        // 3) Start broadcast step
        let broadcast_start = Instant::now();
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let all_shred_bufs: Vec<Vec<u8>> = data_shreds
            .into_iter()
            .chain(coding_shreds.into_iter())
            .map(|s| s.payload)
            .collect();
        trace!("Broadcasting {:?} shreds", all_shred_bufs.len());

        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            &all_shred_bufs,
            &all_seeds,
            stakes.as_ref(),
        )?;

        let broadcast_elapsed = broadcast_start.elapsed();

        self.insertion_elapsed += insert_shreds_elapsed.as_millis();
        self.shredding_elapsed += to_shreds_elapsed.as_millis();
        self.broadcast_elapsed += broadcast_elapsed.as_millis();

        if last_tick == bank.max_tick_height() {
            datapoint_info!(
                "broadcast-bank-stats",
                ("slot", bank.slot() as i64, i64),
                ("shredding_time", self.shredding_elapsed as i64, i64),
                ("insertion_time", self.insertion_elapsed as i64, i64),
                ("broadcast_time", self.broadcast_elapsed as i64, i64),
                ("num_shreds", i64::from(latest_shred_index), i64),
                (
                    "slot_broadcast_time",
                    self.slot_broadcast_start.unwrap().elapsed().as_millis() as i64,
                    i64
                ),
            );
            self.insertion_elapsed = 0;
            self.shredding_elapsed = 0;
            self.broadcast_elapsed = 0;
        }

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
            next_shred_index,
        );

        Ok(())
    }
}
