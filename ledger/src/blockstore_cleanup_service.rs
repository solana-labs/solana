//! The `blockstore_cleanup_service` drops older ledger data to limit disk space usage.
//! The service works by counting the number of live data shreds in the ledger; this
//! can be done quickly and should have a fairly stable correlation to actual bytes.
//! Once the shred count (and thus roughly the byte count) reaches a threshold,
//! the services begins removing data in FIFO order.

use {
    crate::{
        blockstore::{Blockstore, PurgeType},
        blockstore_db::{Result as BlockstoreResult, DATA_SHRED_CF},
    },
    solana_measure::measure::Measure,
    solana_sdk::clock::{Slot, DEFAULT_MS_PER_SLOT},
    std::{
        string::ToString,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
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

// Perform blockstore cleanup at this interval to limit the overhead of cleanup
// Cleanup will be considered after the latest root has advanced by this value
const DEFAULT_CLEANUP_SLOT_INTERVAL: u64 = 512;
// The above slot interval can be roughly equated to a time interval. So, scale
// how often we check for cleanup with the interval. Doing so will avoid wasted
// checks when we know that the latest root could not have advanced far enough
//
// Given that the timing of new slots/roots is not exact, divide by 10 to avoid
// a long wait incase a check occurs just before the interval has elapsed
const LOOP_LIMITER: Duration =
    Duration::from_millis(DEFAULT_CLEANUP_SLOT_INTERVAL * DEFAULT_MS_PER_SLOT / 10);

pub struct BlockstoreCleanupService {
    t_cleanup: JoinHandle<()>,
}

impl BlockstoreCleanupService {
    pub fn new(blockstore: Arc<Blockstore>, max_ledger_shreds: u64, exit: Arc<AtomicBool>) -> Self {
        let mut last_purge_slot = 0;
        let mut last_check_time = Instant::now();

        let t_cleanup = Builder::new()
            .name("solBstoreClean".to_string())
            .spawn(move || {
                info!(
                    "BlockstoreCleanupService has started with max \
                    ledger shreds={max_ledger_shreds}",
                );
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    if last_check_time.elapsed() > LOOP_LIMITER {
                        Self::cleanup_ledger(
                            &blockstore,
                            max_ledger_shreds,
                            &mut last_purge_slot,
                            DEFAULT_CLEANUP_SLOT_INTERVAL,
                        );

                        last_check_time = Instant::now();
                    }
                    // Only sleep for 1 second instead of LOOP_LIMITER so that this
                    // thread can respond to the exit flag in a timely manner
                    thread::sleep(Duration::from_secs(1));
                }
                info!("BlockstoreCleanupService has stopped");
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
        blockstore: &Blockstore,
        root: Slot,
        max_ledger_shreds: u64,
    ) -> (bool, Slot, u64) {
        let data_shred_cf_name = DATA_SHRED_CF.to_string();

        let live_files = blockstore
            .live_files_metadata()
            .expect("Blockstore::live_files_metadata()");
        let num_shreds = live_files
            .iter()
            .filter(|live_file| live_file.column_family_name == data_shred_cf_name)
            .map(|file_meta| file_meta.num_entries)
            .sum();

        // Using the difference between the lowest and highest slot seen will
        // result in overestimating the number of slots in the blockstore since
        // there are likely to be some missing slots, such as when a leader is
        // delinquent for their leader slots.
        //
        // With the below calculations, we will then end up underestimating the
        // mean number of shreds per slot present in the blockstore which will
        // result in cleaning more slots than necessary to get us
        // below max_ledger_shreds.
        //
        // Given that the service runs on an interval, this is good because it
        // means that we are building some headroom so the peak number of alive
        // shreds doesn't get too large before the service's next run.
        //
        // Finally, we have a check to make sure that we don't purge any slots
        // newer than the passed in root. This check is practically only
        // relevant when a cluster has extended periods of not rooting slots.
        // With healthy cluster operation, the minimum ledger size ensures
        // that purged slots will be quite old in relation to the newest root.
        let lowest_slot = blockstore.lowest_slot();
        let highest_slot = blockstore
            .highest_slot()
            .expect("Blockstore::highest_slot()")
            .unwrap_or(lowest_slot);
        if highest_slot < lowest_slot {
            error!(
                "Skipping Blockstore cleanup: \
                highest slot {highest_slot} < lowest slot {lowest_slot}",
            );
            return (false, 0, num_shreds);
        }
        // The + 1 ensures we count the correct number of slots. Additionally,
        // it guarantees num_slots >= 1 for the subsequent division.
        let num_slots = highest_slot - lowest_slot + 1;
        let mean_shreds_per_slot = num_shreds / num_slots;
        info!(
            "Blockstore has {num_shreds} alive shreds in slots [{lowest_slot}, {highest_slot}], \
            mean of {mean_shreds_per_slot} shreds per slot",
        );

        if num_shreds <= max_ledger_shreds {
            return (false, 0, num_shreds);
        }

        // Add an extra (mean_shreds_per_slot - 1) in the numerator
        // so that our integer division rounds up
        let num_slots_to_clean = (num_shreds - max_ledger_shreds + mean_shreds_per_slot - 1)
            .checked_div(mean_shreds_per_slot);

        if let Some(num_slots_to_clean) = num_slots_to_clean {
            // Ensure we don't cleanup anything past the last root we saw
            let lowest_cleanup_slot = std::cmp::min(lowest_slot + num_slots_to_clean - 1, root);
            (true, lowest_cleanup_slot, num_shreds)
        } else {
            error!("Skipping Blockstore cleanup: calculated mean of 0 shreds per slot");
            (false, 0, num_shreds)
        }
    }

    /// Checks for new roots and initiates a cleanup if the last cleanup was at
    /// least `purge_interval` slots ago. A cleanup will no-op if the ledger
    /// already has fewer than `max_ledger_shreds`; otherwise, the cleanup will
    /// purge enough slots to get the ledger size below `max_ledger_shreds`.
    ///
    /// # Arguments
    ///
    /// - `max_ledger_shreds`: the number of shreds to keep since the new root.
    /// - `last_purge_slot`: an both an input and output parameter indicating
    ///   the id of the last purged slot.  As an input parameter, it works
    ///   together with `purge_interval` on whether it is too early to perform
    ///   ledger cleanup.  As an output parameter, it will be updated if this
    ///   function actually performs the ledger cleanup.
    /// - `purge_interval`: the minimum slot interval between two ledger
    ///   cleanup.  When the max root fetched from the Blockstore minus
    ///   `last_purge_slot` is fewer than `purge_interval`, the function will
    ///   simply return `Ok` without actually running the ledger cleanup.
    ///   In this case, `purge_interval` will remain unchanged.
    ///
    /// Also see `blockstore::purge_slot`.
    pub fn cleanup_ledger(
        blockstore: &Arc<Blockstore>,
        max_ledger_shreds: u64,
        last_purge_slot: &mut u64,
        purge_interval: u64,
    ) {
        let root = blockstore.max_root();
        if root - *last_purge_slot <= purge_interval {
            return;
        }
        *last_purge_slot = root;
        info!("Looking for Blockstore data to cleanup, latest root: {root}");

        let disk_utilization_pre = blockstore.storage_size();
        let (slots_to_clean, lowest_cleanup_slot, total_shreds) =
            Self::find_slots_to_clean(blockstore, root, max_ledger_shreds);

        if slots_to_clean {
            *blockstore.lowest_cleanup_slot.write().unwrap() = lowest_cleanup_slot;

            let mut purge_time = Measure::start("purge_slots()");
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
            info!("Cleaned up Blockstore data older than slot {lowest_cleanup_slot}. {purge_time}");
        }

        let disk_utilization_post = blockstore.storage_size();
        Self::report_disk_metrics(disk_utilization_pre, disk_utilization_post, total_shreds);
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
        crate::{blockstore::make_many_slot_entries, get_tmp_ledger_path_auto_delete},
    };

    fn flush_blockstore_contents_to_disk(blockstore: Blockstore) -> Blockstore {
        // The find_slots_to_clean() routine uses a method that queries data
        // from RocksDB SST files. On a running validator, these are created
        // fairly regularly as new data is coming in and contents of memory are
        // pushed to disk. In a unit test environment, we aren't pushing nearly
        // enough data for this to happen organically. So, instead open and
        // close the Blockstore which will perform the flush to SSTs.
        let ledger_path = blockstore.ledger_path().clone();
        drop(blockstore);
        Blockstore::open(&ledger_path).unwrap()
    }

    #[test]
    fn test_find_slots_to_clean() {
        // BlockstoreCleanupService::find_slots_to_clean() does not modify the
        // Blockstore, so we can make repeated calls on the same slots
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Construct and build some shreds for slots [1, 10]
        let num_slots: u64 = 10;
        let num_entries = 200;
        let (shreds, _) = make_many_slot_entries(1, num_slots, num_entries);
        let shreds_per_slot = (shreds.len() / num_slots as usize) as u64;
        assert!(shreds_per_slot > 1);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        // Initiate a flush so inserted shreds found by find_slots_to_clean()
        let blockstore = Arc::new(flush_blockstore_contents_to_disk(blockstore));

        // Ensure no cleaning of slots > last_root
        let last_root = 0;
        let max_ledger_shreds = 0;
        let (should_clean, lowest_purged, _) = BlockstoreCleanupService::find_slots_to_clean(
            &blockstore,
            last_root,
            max_ledger_shreds,
        );
        // Slot 0 will exist in blockstore with zero shreds since it is slot
        // 1's parent. Thus, slot 0 will be identified for clean.
        assert!(should_clean && lowest_purged == 0);
        // Now, set max_ledger_shreds to 1, slot 0 still eligible for clean
        let max_ledger_shreds = 1;
        let (should_clean, lowest_purged, _) = BlockstoreCleanupService::find_slots_to_clean(
            &blockstore,
            last_root,
            max_ledger_shreds,
        );
        assert!(should_clean && lowest_purged == 0);

        // Ensure no cleaning if blockstore contains fewer than max_ledger_shreds
        let last_root = num_slots;
        let max_ledger_shreds = (shreds_per_slot * num_slots) + 1;
        let (should_clean, lowest_purged, _) = BlockstoreCleanupService::find_slots_to_clean(
            &blockstore,
            last_root,
            max_ledger_shreds,
        );
        assert!(!should_clean && lowest_purged == 0);

        for slot in 1..=num_slots {
            // Set last_root to make slots <= slot eligible for cleaning
            let last_root = slot;
            // Set max_ledger_shreds to 0 so that all eligible slots are cleaned
            let max_ledger_shreds = 0;
            let (should_clean, lowest_purged, _) = BlockstoreCleanupService::find_slots_to_clean(
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
            let (should_clean, lowest_purged, _) = BlockstoreCleanupService::find_slots_to_clean(
                &blockstore,
                last_root,
                max_ledger_shreds,
            );
            assert!(should_clean && lowest_purged == slot);
        }
    }

    #[test]
    fn test_cleanup() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        // Initiate a flush so inserted shreds found by find_slots_to_clean()
        let blockstore = Arc::new(flush_blockstore_contents_to_disk(blockstore));

        // Mark 50 as a root to kill all but 5 shreds, which will be in the newest slots
        let mut last_purge_slot = 0;
        blockstore.set_roots([50].iter()).unwrap();
        BlockstoreCleanupService::cleanup_ledger(&blockstore, 5, &mut last_purge_slot, 10);
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
            blockstore.set_roots([slot + num_slots].iter()).unwrap();
            BlockstoreCleanupService::cleanup_ledger(
                &blockstore,
                initial_slots,
                &mut last_purge_slot,
                10,
            );
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
