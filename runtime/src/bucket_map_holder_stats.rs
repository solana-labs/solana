use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct BucketMapHolderStats {
    pub get_mem_us: AtomicU64,
    pub gets_from_mem: AtomicU64,
    pub get_missing_us: AtomicU64,
    pub gets_missing: AtomicU64,
    pub items: AtomicU64,
    pub keys: AtomicU64,
    pub deletes: AtomicU64,
}

impl BucketMapHolderStats {
    pub fn report_stats(&self) {
        datapoint_info!(
            "accounts_index",
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
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            ("items", self.items.swap(0, Ordering::Relaxed), i64),
            ("keys", self.keys.swap(0, Ordering::Relaxed), i64),
        );
    }
}
