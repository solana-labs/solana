//! The `ledger_cleanup_service` drops older ledger data to limit disk space usage.
//! The service works by counting the number of live data shreds in the ledger; this
//! can be done quickly and should have a fairly stable correlation to actual bytes.
//! Once the shred count (and thus roughly the byte count) reaches a threshold,
//! the services begins removing data in FIFO order.

use {
    crossbeam_channel::{Receiver, RecvTimeoutError},
    solana_ledger::{
        blockstore::{Blockstore, PurgeType},
        blockstore_db::Result as BlockstoreResult,
    },
    solana_measure::measure::Measure,
    solana_sdk::clock::Slot,
    std::{
        string::ToString,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

// - To try and keep the RocksDB size under 400GB:
//   Seeing about 1600b/shred, using 2000b/shred for margin, so 200m shreds can be stored in 400gb.
//   at 5k shreds/slot at 50k tps, this is 40k slots (~4.4 hours).
//   At idle, 60 shreds/slot this is about 3.33m slots (~15 days)
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
        let exit = exit.clone();
        let mut last_purge_slot = 0;

        info!(
            "LedgerCleanupService active. max ledger shreds={}",
            max_ledger_shreds
        );

        let t_cleanup = Builder::new()
            .name("solLedgerClean".to_string())
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

    /// A helper function to `cleanup_ledger` which returns a tuple of the
    /// following four elements suggesting whether to clean up the ledger:
    ///
    /// Return value (bool, Slot, u64):
    /// - `slots_to_clean` (bool): a boolean value indicating whether there
    /// are any slots to clean.  If true, then `cleanup_ledger` function
    /// will then proceed with the ledger cleanup.
    /// - `lowest_slot_to_purge` (Slot): the lowest slot to purge.  Any
    ///   slot which is older or equal to `lowest_slot_to_purge` will be
    ///   cleaned up.
    /// - `total_shreds` (u64): the total estimated number of shreds before the
    ///   `root`.
    fn find_slots_to_clean(
        blockstore: &Arc<Blockstore>,
        root: Slot,
        max_ledger_shreds: u64,
    ) -> (bool, Slot, u64) {
        let mut total_slots = Vec::new();
        let mut iterate_time = Measure::start("iterate_time");
        let mut total_shreds = 0;
        for (i, (slot, meta)) in blockstore.slot_meta_iterator(0).unwrap().enumerate() {
            if i == 0 {
                debug!("purge: searching from slot: {}", slot);
            }
            // Unrooted slots are not eligible for cleaning
            if slot > root {
                break;
            }
            // This method not exact since non-full slots will have holes
            total_shreds += meta.received;
            total_slots.push((slot, meta.received));
        }
        iterate_time.stop();
        info!(
            "total_slots={} total_shreds={} max_ledger_shreds={}, {}",
            total_slots.len(),
            total_shreds,
            max_ledger_shreds,
            iterate_time
        );
        if total_shreds < max_ledger_shreds {
            return (false, 0, total_shreds);
        }
        let mut num_shreds_to_clean = 0;
        let mut lowest_cleanup_slot = total_slots[0].0;
        for (slot, num_shreds) in total_slots.iter().rev() {
            num_shreds_to_clean += *num_shreds;
            if num_shreds_to_clean > max_ledger_shreds {
                lowest_cleanup_slot = *slot;
                break;
            }
        }

        (true, lowest_cleanup_slot, total_shreds)
    }

    fn receive_new_roots(new_root_receiver: &Receiver<Slot>) -> Result<Slot, RecvTimeoutError> {
        let root = new_root_receiver.recv_timeout(Duration::from_secs(1))?;
        // Get the newest root
        Ok(new_root_receiver.try_iter().last().unwrap_or(root))
    }

    /// Checks for new roots and initiates a cleanup if the last cleanup was at
    /// least `purge_interval` slots ago. A cleanup will no-op if the ledger
    /// already has fewer than `max_ledger_shreds`; otherwise, the cleanup will
    /// purge enough slots to get the ledger size below `max_ledger_shreds`.
    ///
    /// # Arguments
    ///
    /// - `new_root_receiver`: signal receiver which contains the information
    ///   about what `Slot` is the current root.
    /// - `max_ledger_shreds`: the number of shreds to keep since the new root.
    /// - `last_purge_slot`: an both an input and output parameter indicating
    ///   the id of the last purged slot.  As an input parameter, it works
    ///   together with `purge_interval` on whether it is too early to perform
    ///   ledger cleanup.  As an output parameter, it will be updated if this
    ///   function actually performs the ledger cleanup.
    /// - `purge_interval`: the minimum slot interval between two ledger
    ///   cleanup.  When the root derived from `new_root_receiver` minus
    ///   `last_purge_slot` is fewer than `purge_interval`, the function will
    ///   simply return `Ok` without actually running the ledger cleanup.
    ///   In this case, `purge_interval` will remain unchanged.
    ///
    /// Also see `blockstore::purge_slot`.
    pub fn cleanup_ledger(
        new_root_receiver: &Receiver<Slot>,
        blockstore: &Arc<Blockstore>,
        max_ledger_shreds: u64,
        last_purge_slot: &mut u64,
        purge_interval: u64,
    ) -> Result<(), RecvTimeoutError> {
        let root = Self::receive_new_roots(new_root_receiver)?;
        if root - *last_purge_slot <= purge_interval {
            return Ok(());
        }

        let disk_utilization_pre = blockstore.storage_size();
        info!(
            "purge: last_root={}, last_purge_slot={}, purge_interval={}, disk_utilization={:?}",
            root, last_purge_slot, purge_interval, disk_utilization_pre
        );

        *last_purge_slot = root;

        let (slots_to_clean, lowest_cleanup_slot, total_shreds) =
            Self::find_slots_to_clean(blockstore, root, max_ledger_shreds);

        if slots_to_clean {
            let purge_complete = Arc::new(AtomicBool::new(false));
            let blockstore = blockstore.clone();
            let purge_complete1 = purge_complete.clone();
            let _t_purge = Builder::new()
                .name("solLedgerPurge".to_string())
                .spawn(move || {
                    let mut slot_update_time = Measure::start("slot_update");
                    *blockstore.lowest_cleanup_slot.write().unwrap() = lowest_cleanup_slot;
                    slot_update_time.stop();

                    info!("purging data older than {}", lowest_cleanup_slot);

                    let mut purge_time = Measure::start("purge_slots");

                    // purge any slots older than lowest_cleanup_slot.
                    blockstore.purge_slots(0, lowest_cleanup_slot, PurgeType::CompactionFilter);
                    // Update only after purge operation.
                    // Safety: This value can be used by compaction_filters shared via Arc<AtomicU64>.
                    // Compactions are async and run as a multi-threaded background job. However, this
                    // shouldn't cause consistency issues for iterators and getters because we have
                    // already expired all affected keys (older than or equal to lowest_cleanup_slot)
                    // by the above `purge_slots`. According to the general RocksDB design where SST
                    // files are immutable, even running iterators aren't affected; the database grabs
                    // a snapshot of the live set of sst files at iterator's creation.
                    // Also, we passed the PurgeType::CompactionFilter, meaning no delete_range for
                    // transaction_status and address_signatures CFs. These are fine because they
                    // don't require strong consistent view for their operation.
                    blockstore.set_max_expired_slot(lowest_cleanup_slot);

                    purge_time.stop();
                    info!("{}", purge_time);

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
    use {
        super::*,
        crossbeam_channel::unbounded,
        solana_ledger::{blockstore::make_many_slot_entries, get_tmp_ledger_path_auto_delete},
    };

    #[test]
    fn test_find_slots_to_clean() {
        // LedgerCleanupService::find_slots_to_clean() does not modify the
        // Blockstore, so we can make repeated calls on the same slots
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());

        // Construct and build some shreds for slots [1, 10]
        let num_slots: u64 = 10;
        let num_entries = 200;
        let (shreds, _) = make_many_slot_entries(1, num_slots, num_entries);
        let shreds_per_slot = (shreds.len() / num_slots as usize) as u64;
        assert!(shreds_per_slot > 1);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        // Ensure no cleaning of slots > last_root
        let last_root = 0;
        let max_ledger_shreds = 0;
        let (should_clean, lowest_purged, _) =
            LedgerCleanupService::find_slots_to_clean(&blockstore, last_root, max_ledger_shreds);
        // Slot 0 will exist in blockstore with zero shreds since it is slot
        // 1's parent. Thus, slot 0 will be identified for clean.
        assert!(should_clean && lowest_purged == 0);
        // Now, set max_ledger_shreds to 1 so that slot 0 is left alone
        let max_ledger_shreds = 1;
        let (should_clean, lowest_purged, _) =
            LedgerCleanupService::find_slots_to_clean(&blockstore, last_root, max_ledger_shreds);
        assert!(!should_clean && lowest_purged == 0);

        // Ensure no cleaning if blockstore contains fewer than max_ledger_shreds
        let last_root = num_slots;
        let max_ledger_shreds = (shreds_per_slot * num_slots) + 1;
        let (should_clean, lowest_purged, _) =
            LedgerCleanupService::find_slots_to_clean(&blockstore, last_root, max_ledger_shreds);
        assert!(!should_clean && lowest_purged == 0);

        for slot in 1..=num_slots {
            // Set last_root to make slots <= slot eligible for cleaning
            let last_root = slot;
            // Set max_ledger_shreds to 0 so that all eligible slots are cleaned
            let max_ledger_shreds = 0;
            let (should_clean, lowest_purged, _) = LedgerCleanupService::find_slots_to_clean(
                &blockstore,
                last_root,
                max_ledger_shreds,
            );
            assert!(should_clean && lowest_purged == slot);

            // Set last_root to make all slots eligible for cleaning
            let last_root = num_slots + 1;
            // Set max_ledger_shreds to the number of shreds in slots > slot.
            // This will make it so that slots [1, slot] are cleaned
            let max_ledger_shreds = shreds_per_slot * (num_slots - slot);
            let (should_clean, lowest_purged, _) = LedgerCleanupService::find_slots_to_clean(
                &blockstore,
                last_root,
                max_ledger_shreds,
            );
            assert!(should_clean && lowest_purged == slot);
        }
    }

    #[test]
    fn test_cleanup1() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blockstore.insert_shreds(shreds, None, false).unwrap();
        let blockstore = Arc::new(blockstore);
        let (sender, receiver) = unbounded();

        //send a signal to kill all but 5 shreds, which will be in the newest slots
        let mut last_purge_slot = 0;
        sender.send(50).unwrap();
        LedgerCleanupService::cleanup_ledger(&receiver, &blockstore, 5, &mut last_purge_slot, 10)
            .unwrap();
        assert_eq!(last_purge_slot, 50);

        //check that 0-40 don't exist
        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| assert!(slot > 40));
    }

    #[test]
    fn test_cleanup_speed() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());
        let (sender, receiver) = unbounded();

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
    }
}
