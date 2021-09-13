use crate::accounts_index::IsCached;
use crate::bucket_map_holder::{BucketMapHolder, BucketMapWithEntryType};
use solana_bucket_map::bucket_map::BucketMapDistribution;
use solana_measure::measure::Measure;
use solana_sdk::timing::AtomicInterval;
use std::fmt::Debug;
use std::sync::atomic::Ordering;

use std::sync::atomic::{AtomicBool, AtomicU64};

#[derive(Debug, Default)]
pub struct BucketMapHolderStats {
    last_report: AtomicInterval,
    full_report: AtomicInterval,
    pub get_disk_us: AtomicU64,
    pub update_dist_us: AtomicU64,
    pub get_cache_us: AtomicU64,
    pub update_cache_us: AtomicU64,
    pub dirty_scans: AtomicU64,
    pub age_scans: AtomicU64,
    pub skipped_scans: AtomicU64,
    pub cache_updates: AtomicU64,
    pub cache_upserts: AtomicU64,
    pub items_to_keep: AtomicU64,
    pub bg_flush_cycles: AtomicU64,
    pub bucket_flush_calls: AtomicU64,
    pub aged_count: AtomicU64,
    pub dirty_purge_count: AtomicU64,
    pub gets_from_disk: AtomicU64,
    pub gets_from_disk_empty: AtomicU64,
    pub gets_from_cache: AtomicU64,
    pub flush_entries_skipped_range: AtomicU64,
    pub flush_entries_skipped_stop_flush: AtomicU64,
    pub updates: AtomicU64,
    pub flush_scan_us: AtomicU64,
    pub flush_upsert_us: AtomicU64,
    pub flush_update_us: AtomicU64,
    pub flush_purge_us: AtomicU64,
    pub using_empty_get: AtomicU64,
    pub insert_without_lookup: AtomicU64,
    pub updates_in_cache: AtomicU64,
    pub addrefs: AtomicU64,
    pub unrefs: AtomicU64,
    pub range: AtomicU64,
    pub throughput: AtomicU64,
    pub range_us: AtomicU64,
    pub gathering_stats: AtomicBool,
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
    pub age: AtomicU64,
    pub age_incs: AtomicU64,
    pub age_elapsed_us: AtomicU64,
    pub stats_elapsed_us: AtomicU64,
    pub non_age_elapsed_us: AtomicU64,
}

impl BucketMapHolderStats {
    pub fn report_stats<V: IsCached>(
        &self,
        in_mem_only: bool,
        disk: &BucketMapWithEntryType<V>,
        holder: &BucketMapHolder<V>,
    ) -> bool {
        if self.gathering_stats.load(Ordering::Relaxed) {
            return false;
        }
        if !self.last_report.should_update(1000) {
            return false;
        }
        let mut m = Measure::start("stats");
        self.gathering_stats.store(true, Ordering::Relaxed);

        let cts = holder
            .per_bin
            .iter()
            .map(|x| x.cache.read().unwrap().len())
            .collect::<Vec<_>>();
        let mut ct = 0;
        let mut sum = 0;
        let mut min = usize::MAX;
        let mut max = 0;
        let mut sumd = 0;
        let mut mind = usize::MAX;
        let mut maxd = 0;
        let mut min_cache = usize::MAX;
        let mut max_cache = 0;
        for d in &cts {
            let d = *d;
            ct += d;
            min_cache = std::cmp::min(min_cache, d);
            max_cache = std::cmp::max(max_cache, d);
        }

        let dist = if !in_mem_only {
            // only request the disk stats once per 10s
            if self.full_report.should_update(10_000) {
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
                min = 0;
                max = 0;
                sum = 0;
                sumd = 0;
                mind = 0;
                maxd = 0;
                BucketMapDistribution::default()
            }
        } else {
            for d in holder.per_bin.iter().map(|x| &x.cache) {
                let d = d.read().unwrap().len();
                sum += d;
                min = std::cmp::min(min, d);
                max = std::cmp::max(max, d);
            }
            BucketMapDistribution::default()
        };

        let per_bin_count = holder.per_bin.iter().map(|x| x.count()).sum::<usize>();

        datapoint_info!(
            "accounts_index",
            ("items_in_cache", ct, i64),
            ("min", min, i64),
            ("max", max, i64),
            ("sum_overall", per_bin_count, i64),
            ("sum", sum, i64),
            ("index_entries_allocated", dist.index_entries_allocated, i64),
            ("min_cache_bin", min_cache, i64),
            ("max_cache_bin", max_cache, i64),
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
            (
                "flush_entries_skipped_range",
                self.flush_entries_skipped_range.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_entries_skipped_stop_flush",
                self.flush_entries_skipped_stop_flush
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_scan_us",
                self.flush_scan_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_upsert_us",
                self.flush_upsert_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_update_us",
                self.flush_update_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flush_purge_us",
                self.flush_purge_us.swap(0, Ordering::Relaxed),
                i64
            ),
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
            ("throughput", self.throughput.load(Ordering::Relaxed), i64),
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
                "items_to_keep",
                self.items_to_keep.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "cache_upserts",
                self.cache_upserts.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "cache_updates",
                self.cache_updates.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bin_flush_calls",
                self.bucket_flush_calls.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "aged_count",
                self.aged_count.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "dirty_purge_count",
                self.dirty_purge_count.swap(0, Ordering::Relaxed),
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
            ("age_scans", self.age_scans.swap(0, Ordering::Relaxed), i64),
            (
                "dirty_scans",
                self.dirty_scans.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "skipped_scans",
                self.skipped_scans.swap(0, Ordering::Relaxed),
                i64
            ),
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
            ("age", self.age.load(Ordering::Relaxed), i64),
            ("age_incs", self.age_incs.swap(0, Ordering::Relaxed), i64),
            (
                "age_elapsed_us",
                self.age_elapsed_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "non_age_elapsed_us",
                self.age_elapsed_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "stats_elapsed_us",
                self.stats_elapsed_us.swap(0, Ordering::Relaxed),
                i64
            ),
        );
        m.stop();
        self.stats_elapsed_us
            .fetch_add(m.as_us(), Ordering::Relaxed);
        self.gathering_stats.store(false, Ordering::Relaxed);
        true
    }
}
