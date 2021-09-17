use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct BucketMapHolderStats {
    pub get_mem_us: AtomicU64,
    pub gets_from_mem: AtomicU64,
    pub get_missing_us: AtomicU64,
    pub gets_missing: AtomicU64,
    pub entry_mem_us: AtomicU64,
    pub entries_from_mem: AtomicU64,
    pub entry_missing_us: AtomicU64,
    pub entries_missing: AtomicU64,
    pub updates_in_mem: AtomicU64,
    pub items: AtomicU64,
    pub keys: AtomicU64,
    pub deletes: AtomicU64,
    pub inserts: AtomicU64,
    pub count_in_mem: AtomicU64,
    pub per_bucket_count: Vec<AtomicU64>,
    pub get_range_us: AtomicU64,
}

impl BucketMapHolderStats {
    pub fn new(bins: usize) -> BucketMapHolderStats {
        BucketMapHolderStats {
            per_bucket_count: (0..bins)
                .into_iter()
                .map(|_| AtomicU64::default())
                .collect(),
            ..BucketMapHolderStats::default()
        }
    }

    pub fn insert_or_delete(&self, insert: bool, bin: usize) {
        let per_bucket = self.per_bucket_count.get(bin);
        if insert {
            self.inserts.fetch_add(1, Ordering::Relaxed);
            self.count_in_mem.fetch_add(1, Ordering::Relaxed);
            per_bucket.map(|count| count.fetch_add(1, Ordering::Relaxed));
        } else {
            self.deletes.fetch_add(1, Ordering::Relaxed);
            self.count_in_mem.fetch_sub(1, Ordering::Relaxed);
            per_bucket.map(|count| count.fetch_sub(1, Ordering::Relaxed));
        }
    }

    pub fn report_stats(&self) {
        let mut ct = 0;
        let mut min = usize::MAX;
        let mut max = 0;
        for d in &self.per_bucket_count {
            let d = d.load(Ordering::Relaxed) as usize;
            ct += d;
            min = std::cmp::min(min, d);
            max = std::cmp::max(max, d);
        }

        datapoint_info!(
            "accounts_index",
            (
                "count_in_mem",
                self.count_in_mem.load(Ordering::Relaxed),
                i64
            ),
            ("min_in_bin", min, i64),
            ("max_in_bin", max, i64),
            ("count_from_bins", ct, i64),
            (
                "gets_from_mem",
                self.gets_from_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_mem_us",
                self.get_mem_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "gets_missing",
                self.gets_missing.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_missing_us",
                self.get_missing_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "entries_from_mem",
                self.entries_from_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entry_mem_us",
                self.entry_mem_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "entries_missing",
                self.entries_missing.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "entry_missing_us",
                self.entry_missing_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "updates_in_mem",
                self.updates_in_mem.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_range_us",
                self.get_range_us.swap(0, Ordering::Relaxed),
                i64
            ),
            ("inserts", self.inserts.swap(0, Ordering::Relaxed), i64),
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            ("items", self.items.swap(0, Ordering::Relaxed), i64),
            ("keys", self.keys.swap(0, Ordering::Relaxed), i64),
        );
    }
}
