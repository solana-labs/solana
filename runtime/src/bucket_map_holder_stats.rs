use crate::accounts_index::IsCached;
use crate::bucket_map_holder::CacheSlice;
use solana_measure::measure::Measure;
use solana_sdk::timing::AtomicInterval;
use std::fmt::Debug;
use std::sync::atomic::Ordering;

use std::sync::atomic::AtomicU64;

#[derive(Debug, Default)]
pub struct BucketMapHolderStats {
    last_report: AtomicInterval,
    pub get_cache_us: AtomicU64,
    pub update_cache_us: AtomicU64,
    pub gets_from_disk_empty: AtomicU64,
    pub gets_from_cache: AtomicU64,
    pub updates_in_cache: AtomicU64,
    pub range: AtomicU64,
    pub keys: AtomicU64,
    pub inserts: AtomicU64,
    pub deletes: AtomicU64,
}

impl BucketMapHolderStats {
    pub fn report_stats<V: IsCached>(&self, cache: CacheSlice<V>) -> bool {
        if !self.last_report.should_update(1000) {
            return false;
        }
        let mut m = Measure::start("stats");
        let ct = cache.iter().map(|x| x.read().unwrap().len()).sum::<usize>();
        let mut sum = 0;
        let mut min = usize::MAX;
        let mut max = 0;
        for d in cache {
            let d = d.read().unwrap().len();
            sum += d;
            min = std::cmp::min(min, d);
            max = std::cmp::max(max, d);
        }
        datapoint_info!(
            "accounts_index",
            ("items_in_cache", ct, i64),
            ("min", min, i64),
            ("max", max, i64),
            ("sum", sum, i64),
            (
                "updates_in_cache",
                self.updates_in_cache.swap(0, Ordering::Relaxed),
                i64
            ),
            ("inserts", self.inserts.swap(0, Ordering::Relaxed), i64),
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            (
                "gets_from_disk_empty",
                self.gets_from_disk_empty.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "gets_from_cache",
                self.gets_from_cache.swap(0, Ordering::Relaxed),
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
            ("range", self.range.swap(0, Ordering::Relaxed), i64),
            ("keys", self.keys.swap(0, Ordering::Relaxed), i64),
        );
        m.stop();
        true
    }
}
