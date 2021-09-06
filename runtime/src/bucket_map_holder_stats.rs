use crate::accounts_index::IsCached;
use crate::bucket_map_holder::{BucketMapWithEntryType, CacheSlice};
use solana_bucket_map::bucket_map::BucketMapDistribution;
use solana_sdk::timing::AtomicInterval;
use std::fmt::Debug;
use std::sync::atomic::Ordering;

use std::sync::atomic::AtomicU64;

#[derive(Debug, Default)]
pub struct BucketMapHolderStats {
    last_report: AtomicInterval,
    pub get_disk_us: AtomicU64,
    pub update_dist_us: AtomicU64,
    pub get_cache_us: AtomicU64,
    pub update_cache_us: AtomicU64,
    pub cache_flushes: AtomicU64,
    pub bg_flush_cycles: AtomicU64,
    pub bucket_flush_calls: AtomicU64,
    pub get_purges: AtomicU64,
    pub gets_from_disk: AtomicU64,
    pub gets_from_disk_empty: AtomicU64,
    pub gets_from_cache: AtomicU64,
    pub updates: AtomicU64,
    pub flush0: AtomicU64,
    pub flush1: AtomicU64,
    pub flush2: AtomicU64,
    pub using_empty_get: AtomicU64,
    pub insert_without_lookup: AtomicU64,
    pub updates_in_cache: AtomicU64,
    pub addrefs: AtomicU64,
    pub unrefs: AtomicU64,
    pub range: AtomicU64,
    pub range_us: AtomicU64,
    pub keys: AtomicU64,
    pub inserts: AtomicU64,
    pub inserts_without_checking_disk: AtomicU64,
    pub deletes: AtomicU64,
    pub bins_flushed: AtomicU64,
    pub active_flushes: AtomicU64,
    pub active_flush_threads: AtomicU64,
    pub flushing_idle_us: AtomicU64,
    pub strong_count_no_flush_dirty: AtomicU64,
    pub strong_count_no_purge: AtomicU64,
}

impl BucketMapHolderStats {
    pub fn report_stats<V: IsCached>(
        &self,
        in_mem_only: bool,
        disk: &BucketMapWithEntryType<V>,
        cache: CacheSlice<V>,
    ) {
        if !self.last_report.should_update(1000) {
            return;
        }

        let ct = cache.iter().map(|x| x.read().unwrap().len()).sum::<usize>();
        let mut sum = 0;
        let mut min = usize::MAX;
        let mut max = 0;
        let mut sumd = 0;
        let mut mind = usize::MAX;
        let mut maxd = 0;
        let dist = if !in_mem_only {
            let dist = disk.distribution();
            for d in &dist.sizes {
                let d = *d;
                sum += d;
                min = std::cmp::min(min, d);
                max = std::cmp::max(max, d);
            }
            for d in &dist.data_sizes {
                let d = *d;
                sumd += d;
                mind = std::cmp::min(min, d);
                maxd = std::cmp::max(max, d);
            }
            dist
        } else {
            for d in cache {
                let d = d.read().unwrap().len();
                sum += d;
                min = std::cmp::min(min, d);
                max = std::cmp::max(max, d);
            }
            BucketMapDistribution::default()
        };
        datapoint_info!(
            "accounts_index",
            ("items_in_cache", ct, i64),
            ("min", min, i64),
            ("max", max, i64),
            ("sum", sum, i64),
            ("index_entries_allocated", dist.index_entries_allocated, i64),
            ("mind", mind, i64),
            ("maxd", maxd, i64),
            ("sumd", sumd, i64),
            ("data_entries_allocated", dist.data_entries_allocated, i64),
            ("data_q0", dist.quartiles.0, i64),
            ("data_q1", dist.quartiles.1, i64),
            ("data_q2", dist.quartiles.2, i64),
            ("data_q3", dist.quartiles.3, i64),
            (
                "updates_not_in_cache",
                self.updates.swap(0, Ordering::Relaxed),
                i64
            ),
            ("flush0", self.flush0.swap(0, Ordering::Relaxed), i64),
            ("flush1", self.flush1.swap(0, Ordering::Relaxed), i64),
            ("flush2", self.flush2.swap(0, Ordering::Relaxed), i64),
            (
                "bg_flush_cycles",
                self.bg_flush_cycles.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "updates_in_cache",
                self.updates_in_cache.swap(0, Ordering::Relaxed),
                i64
            ),
            ("inserts", self.inserts.swap(0, Ordering::Relaxed), i64),
            (
                "inserts_without_checking_disk",
                self.inserts_without_checking_disk
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            (
                "using_empty_get",
                self.using_empty_get.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "insert_without_lookup",
                self.insert_without_lookup.swap(0, Ordering::Relaxed),
                i64
            ),
            //("insert_without_lookup", self.insert_without_lookup.load(Ordering::Relaxed), i64),
            (
                "gets_from_disk_empty",
                self.gets_from_disk_empty.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "gets_from_disk",
                self.gets_from_disk.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "gets_from_cache",
                self.gets_from_cache.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_index_resizes",
                disk.stats.index.resizes.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_index_resize_us",
                disk.stats.index.resize_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_index_max",
                *disk.stats.index.max_size.lock().unwrap(),
                i64
            ),
            (
                "bucket_index_new_file_us",
                disk.stats.index.new_file_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_flush_file_us_us",
                disk.stats.index.flush_file_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_mmap_us",
                disk.stats.index.mmap_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_data_resizes",
                disk.stats.data.resizes.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_data_resize_us",
                disk.stats.data.resize_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_data_max",
                *disk.stats.data.max_size.lock().unwrap(),
                i64
            ),
            (
                "bucket_data_new_file_us",
                disk.stats.data.new_file_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flushes",
                self.cache_flushes.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bin_flush_calls",
                self.bucket_flush_calls.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_purges",
                self.get_purges.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_disk_us",
                self.get_disk_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "update_dist_us",
                self.update_dist_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "get_cache_us",
                self.get_cache_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "update_cache_us",
                self.update_cache_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            ("addrefs", self.addrefs.swap(0, Ordering::Relaxed), i64),
            ("unrefs", self.unrefs.swap(0, Ordering::Relaxed), i64),
            ("range", self.range.swap(0, Ordering::Relaxed), i64),
            ("range_us", self.range_us.swap(0, Ordering::Relaxed), i64),
            ("keys", self.keys.swap(0, Ordering::Relaxed), i64),
            ("get", self.updates.swap(0, Ordering::Relaxed), i64),
            (
                "bins_flushed",
                self.bins_flushed.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "active_flushes",
                self.active_flushes.load(Ordering::Relaxed),
                i64
            ),
            (
                "active_flush_threads",
                self.active_flush_threads.load(Ordering::Relaxed),
                i64
            ),
            (
                "flushing_idle_us",
                self.flushing_idle_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "strong_count_no_flush_dirty",
                self.strong_count_no_flush_dirty.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "strong_count_no_purge",
                self.strong_count_no_purge.swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}
