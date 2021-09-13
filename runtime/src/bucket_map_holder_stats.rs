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
            ("inserts", self.inserts.swap(0, Ordering::Relaxed), i64),
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            ("items", self.items.swap(0, Ordering::Relaxed), i64),
            ("keys", self.keys.swap(0, Ordering::Relaxed), i64),
        );
    }
}
