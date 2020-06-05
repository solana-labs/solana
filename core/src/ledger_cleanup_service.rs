//! The `ledger_cleanup_service` drops older ledger data to limit disk space usage

use solana_ledger::blockstore::{Blockstore, PurgeType};
use solana_ledger::blockstore_db::Result as BlockstoreResult;
use solana_measure::measure::Measure;
use solana_sdk::clock::{Slot, DEFAULT_TICKS_PER_SLOT, TICKS_PER_DAY};
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

// - To try and keep the RocksDB size under 400GB:
//   Seeing about 1600b/shred, using 2000b/shred for margin, so 200m shreds can be stored in 400gb.
//   at 5k shreds/slot at 50k tps, this is 500k slots (~5 hours).
//   At idle, 60 shreds/slot this is about 4m slots (18 days)
// This is chosen to allow enough time for
// - A validator to download a snapshot from a peer and boot from it
// - To make sure that if a validator needs to reboot from its own snapshot, it has enough slots locally
//   to catch back up to where it was when it stopped
pub const DEFAULT_MAX_LEDGER_SHREDS: u64 = 200_000_000;

// Allow down to 50m, or 3.5 days at idle, 1hr at 50k load, around ~100GB
pub const DEFAULT_MIN_MAX_LEDGER_SHREDS: u64 = 50_000_000;

// Check for removing slots at this interval so we don't purge too often
// and starve other blockstore users.
pub const DEFAULT_PURGE_SLOT_INTERVAL: u64 = 512;

// Delay between purges to cooperate with other blockstore users
pub const DEFAULT_DELAY_BETWEEN_PURGES: Duration = Duration::from_millis(500);

// Compacting at a slower interval than purging helps keep IOPS down.
// Once a day should be ample
const DEFAULT_COMPACTION_SLOT_INTERVAL: u64 = TICKS_PER_DAY / DEFAULT_TICKS_PER_SLOT;

pub struct LedgerCleanupService {
    t_cleanup: JoinHandle<()>,
}

impl LedgerCleanupService {
    pub fn new(
        new_root_receiver: Receiver<Slot>,
        blockstore: Arc<Blockstore>,
        max_ledger_shreds: u64,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        info!(
            "LedgerCleanupService active. Max Ledger Slots {}",
            max_ledger_shreds
        );
        let exit = exit.clone();
        let mut last_purge_slot = 0;
        let mut last_compaction_slot = 0;

        let t_cleanup = Builder::new()
            .name("solana-ledger-cleanup".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::cleanup_ledger(
                    &new_root_receiver,
                    &blockstore,
                    max_ledger_shreds,
                    &mut last_purge_slot,
                    DEFAULT_PURGE_SLOT_INTERVAL,
                    Some(DEFAULT_DELAY_BETWEEN_PURGES),
                    &mut last_compaction_slot,
                    DEFAULT_COMPACTION_SLOT_INTERVAL,
                ) {
                    match e {
                        RecvTimeoutError::Disconnected => break,
                        RecvTimeoutError::Timeout => (),
                    }
                }
            })
            .unwrap();
        Self { t_cleanup }
    }

    fn find_slots_to_clean(
        blockstore: &Arc<Blockstore>,
        root: Slot,
        max_ledger_shreds: u64,
    ) -> (bool, Slot, Slot, u64) {
        let mut total_slots = Vec::new();
        let mut iterate_time = Measure::start("iterate_time");
        let mut total_shreds = 0;
        let mut first_slot = 0;
        for (i, (slot, meta)) in blockstore.slot_meta_iterator(0).unwrap().enumerate() {
            if i == 0 {
                first_slot = slot;
                debug!("purge: searching from slot: {}", slot);
            }
            // Not exact since non-full slots will have holes
            total_shreds += meta.received;
            total_slots.push((slot, meta.received));
            if slot > root {
                break;
            }
        }
        iterate_time.stop();
        info!(
            "first_slot={} total_slots={} total_shreds={} max_ledger_shreds={}, {}",
            first_slot,
            total_slots.len(),
            total_shreds,
            max_ledger_shreds,
            iterate_time
        );
        if (total_shreds as u64) < max_ledger_shreds {
            return (false, 0, 0, total_shreds);
        }
        let mut num_shreds_to_clean = 0;
        let mut lowest_cleanup_slot = total_slots[0].0;
        for (slot, num_shreds) in total_slots.iter().rev() {
            num_shreds_to_clean += *num_shreds as u64;
            if num_shreds_to_clean > max_ledger_shreds {
                lowest_cleanup_slot = *slot;
                break;
            }
        }

        (true, first_slot, lowest_cleanup_slot, total_shreds)
    }

    fn receive_new_roots(new_root_receiver: &Receiver<Slot>) -> Result<Slot, RecvTimeoutError> {
        let mut root = new_root_receiver.recv_timeout(Duration::from_secs(1))?;
        // Get the newest root
        while let Ok(new_root) = new_root_receiver.try_recv() {
            root = new_root;
        }
        Ok(root)
    }

    pub fn cleanup_ledger(
        new_root_receiver: &Receiver<Slot>,
        blockstore: &Arc<Blockstore>,
        max_ledger_shreds: u64,
        last_purge_slot: &mut u64,
        purge_interval: u64,
        delay_between_purges: Option<Duration>,
        last_compaction_slot: &mut u64,
        compaction_interval: u64,
    ) -> Result<(), RecvTimeoutError> {
        let root = Self::receive_new_roots(new_root_receiver)?;
        if root - *last_purge_slot <= purge_interval {
            return Ok(());
        }

        let disk_utilization_pre = blockstore.storage_size();
        info!(
            "purge: last_root={}, last_purge_slot={}, purge_interval={}, last_compaction_slot={}, disk_utilization={:?}",
            root, last_purge_slot, purge_interval, last_compaction_slot, disk_utilization_pre
        );
        *last_purge_slot = root;

        let (slots_to_clean, purge_first_slot, lowest_cleanup_slot, total_shreds) =
            Self::find_slots_to_clean(&blockstore, root, max_ledger_shreds);

        if slots_to_clean {
            let mut compact_first_slot = std::u64::MAX;
            if lowest_cleanup_slot.saturating_sub(*last_compaction_slot) > compaction_interval {
                compact_first_slot = *last_compaction_slot;
                *last_compaction_slot = lowest_cleanup_slot;
            }

            let purge_complete = Arc::new(AtomicBool::new(false));
            let blockstore = blockstore.clone();
            let purge_complete1 = purge_complete.clone();
            let _t_purge = Builder::new()
                .name("solana-ledger-purge".to_string())
                .spawn(move || {
                    let mut slot_update_time = Measure::start("slot_update");
                    *blockstore.lowest_cleanup_slot.write().unwrap() = lowest_cleanup_slot;
                    slot_update_time.stop();

                    info!(
                        "purging data from slots {} to {}",
                        purge_first_slot, lowest_cleanup_slot
                    );

                    let mut purge_time = Measure::start("purge_slots_with_delay");
                    blockstore.purge_slots_with_delay(
                        purge_first_slot,
                        lowest_cleanup_slot,
                        delay_between_purges,
                        PurgeType::PrimaryIndex,
                    );
                    purge_time.stop();
                    info!("{}", purge_time);

                    if compact_first_slot < lowest_cleanup_slot {
                        info!(
                            "compacting data from slots {} to {}",
                            compact_first_slot, lowest_cleanup_slot
                        );
                        if let Err(err) =
                            blockstore.compact_storage(compact_first_slot, lowest_cleanup_slot)
                        {
                            // This error is not fatal and indicates an internal error?
                            error!(
                                "Error: {:?}; Couldn't compact storage from {:?} to {:?}",
                                err, compact_first_slot, lowest_cleanup_slot
                            );
                        }
                    }

                    purge_complete1.store(true, Ordering::Relaxed);
                })
                .unwrap();

            // Keep pulling roots off `new_root_receiver` while purging to avoid channel buildup
            while !purge_complete.load(Ordering::Relaxed) {
                if let Err(err) = Self::receive_new_roots(new_root_receiver) {
                    debug!("receive_new_roots: {}", err);
                }
                thread::sleep(Duration::from_secs(1));
            }
        }

        let disk_utilization_post = blockstore.storage_size();
        Self::report_disk_metrics(disk_utilization_pre, disk_utilization_post, total_shreds);

        Ok(())
    }

    fn report_disk_metrics(
        pre: BlockstoreResult<u64>,
        post: BlockstoreResult<u64>,
        total_shreds: u64,
    ) {
        if let (Ok(pre), Ok(post)) = (pre, post) {
            datapoint_info!(
                "ledger_disk_utilization",
                ("disk_utilization_pre", pre as i64, i64),
                ("disk_utilization_post", post as i64, i64),
                ("disk_utilization_delta", (pre as i64 - post as i64), i64),
                ("total_shreds", total_shreds, i64),
            );
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_cleanup.join()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use solana_ledger::blockstore::make_many_slot_entries;
    use solana_ledger::get_tmp_ledger_path;
    use std::sync::mpsc::channel;

    #[test]
    fn test_cleanup() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blockstore.insert_shreds(shreds, None, false).unwrap();
        let blockstore = Arc::new(blockstore);
        let (sender, receiver) = channel();

        //send a signal to kill all but 5 shreds, which will be in the newest slots
        let mut last_purge_slot = 0;
        let mut last_compaction_slot = 0;
        sender.send(50).unwrap();
        LedgerCleanupService::cleanup_ledger(
            &receiver,
            &blockstore,
            5,
            &mut last_purge_slot,
            10,
            None,
            &mut last_compaction_slot,
            10,
        )
        .unwrap();

        //check that 0-40 don't exist
        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| assert!(slot > 40));

        drop(blockstore);
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_cleanup_speed() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        let mut blockstore = Blockstore::open(&blockstore_path).unwrap();
        blockstore.set_no_compaction(true);
        let blockstore = Arc::new(blockstore);
        let (sender, receiver) = channel();

        let mut first_insert = Measure::start("first_insert");
        let initial_slots = 50;
        let initial_entries = 5;
        let (shreds, _) = make_many_slot_entries(0, initial_slots, initial_entries);
        blockstore.insert_shreds(shreds, None, false).unwrap();
        first_insert.stop();
        info!("{}", first_insert);

        let mut last_purge_slot = 0;
        let mut last_compaction_slot = 0;
        let mut slot = initial_slots;
        let mut num_slots = 6;
        for _ in 0..5 {
            let mut insert_time = Measure::start("insert time");
            let batch_size = 2;
            let batches = num_slots / batch_size;
            for i in 0..batches {
                let (shreds, _) = make_many_slot_entries(slot + i * batch_size, batch_size, 5);
                blockstore.insert_shreds(shreds, None, false).unwrap();
                if i % 100 == 0 {
                    info!("inserting..{} of {}", i, batches);
                }
            }
            insert_time.stop();

            let mut time = Measure::start("purge time");
            sender.send(slot + num_slots).unwrap();
            LedgerCleanupService::cleanup_ledger(
                &receiver,
                &blockstore,
                initial_slots,
                &mut last_purge_slot,
                10,
                None,
                &mut last_compaction_slot,
                10,
            )
            .unwrap();
            time.stop();
            info!(
                "slot: {} size: {} {} {}",
                slot, num_slots, insert_time, time
            );
            slot += num_slots;
            num_slots *= 2;
        }

        drop(blockstore);
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }
}
