use crate::accounts_index::{AccountsIndexConfig, IndexValue};
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::waitable_condvar::WaitableCondvar;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;
pub type Age = u8;

// will eventually hold the bucket map
pub struct BucketMapHolder<T: IndexValue> {
    pub count_ages_flushed: AtomicUsize,
    pub age: AtomicU8,
    pub stats: BucketMapHolderStats,

    // used by bg processing to know when any bucket has become dirty
    pub wait_dirty_bucket: WaitableCondvar,
    next_bucket_to_flush: Mutex<usize>,
    bins: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: IndexValue> Debug for BucketMapHolder<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

#[allow(clippy::mutex_atomic)]
impl<T: IndexValue> BucketMapHolder<T> {
    pub fn increment_age(&self) {
        // fetch_add is defined to wrap.
        // That's what we want. 0..255, then back to 0.
        self.age.fetch_add(1, Ordering::Relaxed);
        // since we changed age, there are now 0 buckets that have been flushed at this age
        let previous = self.count_ages_flushed.swap(0, Ordering::Relaxed);
        assert!(previous >= self.bins); // we should not have increased age before previous age was fully flushed
    }

    pub fn current_age(&self) -> Age {
        self.age.load(Ordering::Relaxed)
    }

    pub fn bucket_flushed_at_current_age(&self) {
        self.count_ages_flushed.fetch_add(1, Ordering::Relaxed);
    }

    // have all buckets been flushed at the current age?
    pub fn all_buckets_flushed_at_current_age(&self) -> bool {
        self.count_ages_flushed.load(Ordering::Relaxed) >= self.bins
    }

    pub fn new(bins: usize, _config: &Option<AccountsIndexConfig>) -> Self {
        Self {
            count_ages_flushed: AtomicUsize::default(),
            age: AtomicU8::default(),
            stats: BucketMapHolderStats::default(),
            wait_dirty_bucket: WaitableCondvar::default(),
            next_bucket_to_flush: Mutex::new(0),
            bins,
            _phantom: std::marker::PhantomData::<T>::default(),
        }
    }

    // get the next bucket to flush, with the idea that the previous bucket
    // is perhaps being flushed by another thread already.
    pub fn next_bucket_to_flush(&self) -> usize {
        // could be lock-free as an optimization
        // wrapping is tricky
        let mut lock = self.next_bucket_to_flush.lock().unwrap();
        let result = *lock;
        *lock = (result + 1) % self.bins;
        result
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_next_bucket_to_flush() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()));
        let visited = (0..bins)
            .into_iter()
            .map(|_| AtomicUsize::default())
            .collect::<Vec<_>>();
        let iterations = bins * 30;
        let threads = bins * 4;
        let expected = threads * iterations / bins;

        (0..threads).into_par_iter().for_each(|_| {
            (0..iterations).into_iter().for_each(|_| {
                let bin = test.next_bucket_to_flush();
                visited[bin].fetch_add(1, Ordering::Relaxed);
            });
        });
        visited.iter().enumerate().for_each(|(bin, visited)| {
            assert_eq!(visited.load(Ordering::Relaxed), expected, "bin: {}", bin)
        });
    }

    #[test]
    fn test_age_increment() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()));
        for age in 0..513 {
            assert_eq!(test.current_age(), (age % 256) as Age);

            // inc all
            for _ in 0..bins {
                assert!(!test.all_buckets_flushed_at_current_age());
                test.bucket_flushed_at_current_age();
            }

            test.increment_age();
        }
    }

    #[test]
    fn test_age_broad() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()));
        assert_eq!(test.current_age(), 0);
        assert!(!test.all_buckets_flushed_at_current_age());
        // inc all but 1
        for _ in 1..bins {
            test.bucket_flushed_at_current_age();
            assert!(!test.all_buckets_flushed_at_current_age());
        }
        test.bucket_flushed_at_current_age();
        assert!(test.all_buckets_flushed_at_current_age());
        test.increment_age();

        assert_eq!(test.current_age(), 1);
        assert!(!test.all_buckets_flushed_at_current_age());
    }
}
