//! The `ledger_cleanup_service` drops older ledger data to limit disk space usage

use solana_ledger::blockstore::Blockstore;
use solana_ledger::blockstore_db::Result as BlockstoreResult;
use solana_measure::measure::Measure;
use solana_metrics::datapoint_debug;
use solana_sdk::clock::Slot;
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

// - To try and keep the RocksDB size under 512GB:
//   Seeing about 1600b/shred, using 2000b/shred for margin, so 250m shreds can be stored in 512gb.
//   at 5k shreds/slot at 50k tps, this is 500k slots (~5.5 hours).
//   At idle, 60 shreds/slot this is about 4m slots (18 days)
// This is chosen to allow enough time for
// - A validator to download a snapshot from a peer and boot from it
// - To make sure that if a validator needs to reboot from its own snapshot, it has enough slots locally
//   to catch back up to where it was when it stopped
pub const DEFAULT_MAX_LEDGER_SHREDS: u64 = 250_000_000;

// Check for removing slots at this interval so we don't purge too often
// and starve other blockstore users.
pub const DEFAULT_PURGE_SLOT_INTERVAL: u64 = 512;

// Remove a limited number of slots at a time, so the operation
// does not take too long and block other blockstore users.
pub const DEFAULT_PURGE_BATCH_SIZE: u64 = 256;

pub struct LedgerCleanupService {
    t_cleanup: JoinHandle<()>,
}

impl LedgerCleanupService {
    pub fn new(
        new_root_receiver: Receiver<Slot>,
        blockstore: Arc<Blockstore>,
        max_ledger_slots: u64,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        info!(
            "LedgerCleanupService active. Max Ledger Slots {}",
            max_ledger_slots
        );
        let exit = exit.clone();
        let mut last_purge_slot = 0;
        let t_cleanup = Builder::new()
            .name("solana-ledger-cleanup".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::cleanup_ledger(
                    &new_root_receiver,
                    &blockstore,
                    max_ledger_slots,
                    &mut last_purge_slot,
                    DEFAULT_PURGE_SLOT_INTERVAL,
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
    ) -> (u64, Slot, Slot) {
        let mut shreds = Vec::new();
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
            shreds.push((slot, meta.received));
            if slot > root {
                break;
            }
        }
        iterate_time.stop();
        info!(
            "checking for ledger purge: max_shreds: {} slots: {} total_shreds: {} {}",
            max_ledger_shreds,
            shreds.len(),
            total_shreds,
            iterate_time
        );
        if (total_shreds as u64) < max_ledger_shreds {
            return (0, 0, 0);
        }
        let mut cur_shreds = 0;
        let mut lowest_slot_to_clean = shreds[0].0;
        for (slot, num_shreds) in shreds.iter().rev() {
            cur_shreds += *num_shreds as u64;
            if cur_shreds > max_ledger_shreds {
                lowest_slot_to_clean = *slot;
                break;
            }
        }

        (cur_shreds, lowest_slot_to_clean, first_slot)
    }

    fn cleanup_ledger(
        new_root_receiver: &Receiver<Slot>,
        blockstore: &Arc<Blockstore>,
        max_ledger_shreds: u64,
        last_purge_slot: &mut u64,
        purge_interval: u64,
    ) -> Result<(), RecvTimeoutError> {
        let mut root = new_root_receiver.recv_timeout(Duration::from_secs(1))?;
        // Get the newest root
        while let Ok(new_root) = new_root_receiver.try_recv() {
            root = new_root;
        }

        if root - *last_purge_slot > purge_interval {
            let disk_utilization_pre = blockstore.storage_size();
            info!(
                "purge: new root: {} last_purge: {} purge_interval: {} disk: {:?}",
                root, last_purge_slot, purge_interval, disk_utilization_pre
            );
            *last_purge_slot = root;

            let (num_shreds_to_clean, lowest_slot_to_clean, mut first_slot) =
                Self::find_slots_to_clean(blockstore, root, max_ledger_shreds);

            if num_shreds_to_clean > 0 {
                debug!(
                    "cleaning up to: {} shreds: {} first: {}",
                    lowest_slot_to_clean, num_shreds_to_clean, first_slot
                );
                loop {
                    let current_lowest =
                        std::cmp::min(lowest_slot_to_clean, first_slot + DEFAULT_PURGE_BATCH_SIZE);

                    let mut slot_update_time = Measure::start("slot_update");
                    *blockstore.lowest_cleanup_slot.write().unwrap() = current_lowest;
                    slot_update_time.stop();

                    let mut clean_time = Measure::start("ledger_clean");
                    blockstore.purge_slots(first_slot, Some(current_lowest));
                    clean_time.stop();

                    debug!(
                        "ledger purge {} -> {}: {} {}",
                        first_slot, current_lowest, slot_update_time, clean_time
                    );
                    first_slot += DEFAULT_PURGE_BATCH_SIZE;
                    if current_lowest == lowest_slot_to_clean {
                        break;
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            }

            let disk_utilization_post = blockstore.storage_size();

            Self::report_disk_metrics(disk_utilization_pre, disk_utilization_post);
        }

        Ok(())
    }

    fn report_disk_metrics(pre: BlockstoreResult<u64>, post: BlockstoreResult<u64>) {
        if let (Ok(pre), Ok(post)) = (pre, post) {
            datapoint_debug!(
                "ledger_disk_utilization",
                ("disk_utilization_pre", pre as i64, i64),
                ("disk_utilization_post", post as i64, i64),
                ("disk_utilization_delta", (pre as i64 - post as i64), i64)
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
        sender.send(50).unwrap();
        LedgerCleanupService::cleanup_ledger(&receiver, &blockstore, 5, &mut last_purge_slot, 10)
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

    #[test]
    fn test_compaction() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&blockstore_path).unwrap());

        let n = 10_000;
        let batch_size = 100;
        let batches = n / batch_size;
        let max_ledger_shreds = 100;

        for i in 0..batches {
            let (shreds, _) = make_many_slot_entries(i * batch_size, batch_size, 1);
            blockstore.insert_shreds(shreds, None, false).unwrap();
        }

        let u1 = blockstore.storage_size().unwrap() as f64;

        // send signal to cleanup slots
        let (sender, receiver) = channel();
        sender.send(n).unwrap();
        let mut next_purge_batch = 0;
        LedgerCleanupService::cleanup_ledger(
            &receiver,
            &blockstore,
            max_ledger_shreds,
            &mut next_purge_batch,
            10,
        )
        .unwrap();

        thread::sleep(Duration::from_secs(2));

        let u2 = blockstore.storage_size().unwrap() as f64;

        assert!(u2 < u1, "insufficient compaction! pre={},post={}", u1, u2,);

        // check that early slots don't exist
        let max_slot = n - max_ledger_shreds - 1;
        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| assert!(slot > max_slot));

        drop(blockstore);
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }
}
