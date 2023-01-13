use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Debug, Default)]
pub struct BucketStats {
    pub resizes: AtomicU64,
    pub max_size: AtomicU64,
    pub resize_us: AtomicU64,
    pub new_file_us: AtomicU64,
    pub flush_file_us: AtomicU64,
    pub mmap_us: AtomicU64,
    pub find_entry_mut_us: AtomicU64,
}

impl BucketStats {
    pub fn update_max_size(&self, size: u64) {
        self.max_size.fetch_max(size, Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub struct BucketMapStats {
    pub index: Arc<BucketStats>,
    pub data: Arc<BucketStats>,
}
