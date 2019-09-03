use super::broadcast_utils;
use super::*;
use crate::shred::Shred;
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
        _broadcast: &mut Broadcast,
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
        let keypair = &cluster_info.read().unwrap().keypair.clone();
        let mut latest_blob_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0);

        let parent_slot = if let Some(parent_bank) = bank.parent() {
            parent_bank.slot()
        } else {
            0
        };
        let mut all_shreds = vec![];
        let mut all_seeds = vec![];
        let num_ventries = receive_results.ventries.len();
        receive_results
            .ventries
            .into_iter()
            .enumerate()
            .for_each(|(i, entries_tuple)| {
                let (entries, _): (Vec<_>, Vec<_>) = entries_tuple.into_iter().unzip();
                //entries
                let mut shredder = Shredder::new(
                    bank.slot(),
                    Some(parent_slot),
                    1.0,
                    keypair,
                    latest_blob_index as u32,
                )
                .expect("Expected to create a new shredder");

                bincode::serialize_into(&mut shredder, &entries)
                    .expect("Expect to write all entries to shreds");
                if i == (num_ventries - 1) && last_tick == bank.max_tick_height() {
                    shredder.finalize_slot();
                } else {
                    shredder.finalize_fec_block();
                }

                let shreds: Vec<Shred> = shredder
                    .shreds
                    .iter()
                    .map(|s| bincode::deserialize(s).unwrap())
                    .collect();

                let mut seeds: Vec<[u8; 32]> = shreds.iter().map(|s| s.seed()).collect();
                trace!("Inserting {:?} shreds in blocktree", shreds.len());
                blocktree
                    .insert_shreds(shreds)
                    .expect("Failed to insert shreds in blocktree");
                latest_blob_index = u64::from(shredder.index);
                all_shreds.append(&mut shredder.shreds);
                all_seeds.append(&mut seeds);
            });

        let to_blobs_elapsed = to_blobs_start.elapsed();

        // 3) Start broadcast step
        let broadcast_start = Instant::now();
        let bank_epoch = bank.get_stakers_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        trace!("Broadcasting {:?} shreds", all_shreds.len());
        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            &all_shreds,
            &all_seeds,
            stakes.as_ref(),
        )?;

        inc_new_counter_debug!("streamer-broadcast-sent", all_shreds.len());

        let broadcast_elapsed = broadcast_start.elapsed();
        self.update_broadcast_stats(
            duration_as_ms(&broadcast_elapsed),
            duration_as_ms(&(receive_elapsed + to_blobs_elapsed + broadcast_elapsed)),
            num_entries,
            duration_as_ms(&to_blobs_elapsed),
            latest_blob_index,
        );

        Ok(())
    }
}
