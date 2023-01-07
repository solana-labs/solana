use std::sync::{atomic::AtomicU64, Arc, Mutex};

#[derive(Debug, Default)]
pub struct BucketStats {
    pub resizes: AtomicU64,
    pub max_size: Mutex<u64>,
    pub resize_us: AtomicU64,
    pub new_file_us: AtomicU64,
    pub flush_file_us: AtomicU64,
    pub mmap_us: AtomicU64,
    pub find_entry_mut_us: AtomicU64,
}

impl BucketStats {
    pub fn update_max_size(&self, size: u64) {
        let mut max = self.max_size.lock().unwrap();
        *max = std::cmp::max(*max, size);
    }
}

#[derive(Debug, Default)]
pub struct BucketMapStats {
    pub index: Arc<BucketStats>,
    pub data: Arc<BucketStats>,
}
