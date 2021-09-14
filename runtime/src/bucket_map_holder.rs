use crate::accounts_index::{AccountMapEntry, IndexValue};
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::waitable_condvar::WaitableCondvar;
use solana_measure::measure::Measure;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::ops::RangeBounds;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
type K = Pubkey;

// will eventually hold the bucket map
#[derive(Default)]
pub struct BucketMapHolder<T: IndexValue> {
    pub stats: Arc<BucketMapHolderStats>,
    pub buckets: Vec<Arc<CachedBucket<T>>>,
}

#[derive(Default)]
pub struct CachedBucket<T: IndexValue> {
    _bin: usize,
    stats: Arc<BucketMapHolderStats>,
    pub map: RwLock<HashMap<Pubkey, AccountMapEntry<T>>>,
}

impl<T: IndexValue> CachedBucket<T> {
    pub fn new(stats: Arc<BucketMapHolderStats>, bin: usize) -> Self {
        Self {
            _bin: bin,
            stats,
            ..Self::default()
        }
    }

    pub fn items<R>(&self, range: &Option<&R>) -> Vec<(K, AccountMapEntry<T>)>
    where
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        Self::update_stat(&self.stats().items, 1);
        let map = self.map().read().unwrap();
        let mut result = Vec::with_capacity(map.len());
        map.iter().for_each(|(k, v)| {
            if range.map(|range| range.contains(k)).unwrap_or(true) {
                result.push((*k, v.clone()));
            }
        });
        result
    }

    fn map(&self) -> &RwLock<HashMap<Pubkey, AccountMapEntry<T>>> {
        &self.map
    }

    fn stats(&self) -> &BucketMapHolderStats {
        &self.stats
    }

    fn update_stat(stat: &AtomicU64, value: u64) {
        if value != 0 {
            stat.fetch_add(value, Ordering::Relaxed);
        }
    }

    pub fn update_time_stat(stat: &AtomicU64, mut m: Measure) {
        m.stop();
        let value = m.as_us();
        Self::update_stat(stat, value);
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
