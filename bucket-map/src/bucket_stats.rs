use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Debug, Default)]
pub struct StartupBucketStats {
    pub entries_created: AtomicU64,
    pub entries_reused: AtomicU64,
}

#[derive(Debug, Default)]
pub struct BucketStats {
    pub resizes: AtomicU64,
    pub failed_resizes: AtomicU64,
    pub max_size: AtomicU64,
    pub resize_us: AtomicU64,
    pub new_file_us: AtomicU64,
    pub flush_file_us: AtomicU64,
    pub mmap_us: AtomicU64,
    pub find_index_entry_mut_us: AtomicU64,
    pub file_count: AtomicU64,
    pub total_file_size: AtomicU64,
    pub startup: StartupBucketStats,
}

impl BucketStats {
    pub fn update_max_size(&self, size: u64) {
        self.max_size.fetch_max(size, Ordering::Relaxed);
    }

    pub fn resize_grow(&self, old_size: u64, new_size: u64) {
        let size_change = new_size.saturating_sub(old_size);
        self.total_file_size
            .fetch_add(size_change, Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub struct BucketMapStats {
    pub index: Arc<BucketStats>,
    pub data: Arc<BucketStats>,
}
