//! Cached data for hashing accounts
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Default, Debug)]
pub struct CacheHashDataStats {
    pub cache_file_size: AtomicUsize,
    pub cache_file_count: AtomicUsize,
    pub total_entries: AtomicUsize,
    pub loaded_from_cache: AtomicUsize,
    pub entries_loaded_from_cache: AtomicUsize,
    pub save_us: AtomicU64,
    pub saved_to_cache: AtomicUsize,
    pub write_to_mmap_us: AtomicU64,
    pub create_save_us: AtomicU64,
    pub load_us: AtomicU64,
    pub read_us: AtomicU64,
    pub unused_cache_files: AtomicUsize,
}

impl CacheHashDataStats {
    pub fn report(&self) {
        datapoint_info!(
            "cache_hash_data_stats",
            (
                "cache_file_size",
                self.cache_file_size.load(Ordering::Relaxed),
                i64
            ),
            (
                "cache_file_count",
                self.cache_file_count.load(Ordering::Relaxed),
                i64
            ),
            (
                "total_entries",
                self.total_entries.load(Ordering::Relaxed),
                i64
            ),
            (
                "loaded_from_cache",
                self.loaded_from_cache.load(Ordering::Relaxed),
                i64
            ),
            (
                "saved_to_cache",
                self.saved_to_cache.load(Ordering::Relaxed),
                i64
            ),
            (
                "entries_loaded_from_cache",
                self.entries_loaded_from_cache.load(Ordering::Relaxed),
                i64
            ),
            ("save_us", self.save_us.load(Ordering::Relaxed), i64),
            (
                "write_to_mmap_us",
                self.write_to_mmap_us.load(Ordering::Relaxed),
                i64
            ),
            (
                "create_save_us",
                self.create_save_us.load(Ordering::Relaxed),
                i64
            ),
            ("load_us", self.load_us.load(Ordering::Relaxed), i64),
            ("read_us", self.read_us.load(Ordering::Relaxed), i64),
            (
                "unused_cache_files",
                self.unused_cache_files.load(Ordering::Relaxed),
                i64
            ),
        );
    }
}
