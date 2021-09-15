use crate::accounts_index::IndexValue;
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::waitable_condvar::WaitableCondvar;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// will eventually hold the bucket map
#[derive(Default)]
pub struct BucketMapHolder<T: IndexValue> {
    pub stats: Arc<BucketMapHolderStats>,
    pub buckets: Vec<Arc<CachedBucket<T>>>,
}

#[derive(Default)]
pub struct CachedBucket<T: IndexValue> {
    // soon, this will hold the hashmap itself: pub map: RwLock<HashMap<Pubkey, AccountMapEntry<T>>>,
    _bin: usize,
    _stats: Arc<BucketMapHolderStats>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: IndexValue> CachedBucket<T> {
    pub fn new(stats: Arc<BucketMapHolderStats>, bin: usize) -> Self {
        Self {
            _bin: bin,
            _stats: stats,
            ..Self::default()
        }
    }
}

impl<T: IndexValue> BucketMapHolder<T> {
    pub fn new(bins: usize) -> Self {
        let stats = Arc::new(BucketMapHolderStats::default());
        let buckets = (0..bins)
            .into_iter()
            .map(|bin| Arc::new(CachedBucket::new(stats.clone(), bin)))
            .collect();
        Self { stats, buckets }
    }

    // intended to execute in a bg thread
    pub fn background(&self, exit: Arc<AtomicBool>, wait: Arc<WaitableCondvar>) {
        loop {
            wait.wait_timeout(Duration::from_millis(10000)); // account index stats every 10 s
            if exit.load(Ordering::Relaxed) {
                break;
            }
            self.stats.report_stats();
        }
    }
}
