use crate::accounts_index::{AccountsIndexConfig, IndexValue};
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::in_mem_accounts_index::{InMemAccountsIndex, SlotT};
use crate::waitable_condvar::WaitableCondvar;
use solana_bucket_map::bucket_map::{BucketMap, BucketMapConfig};
use solana_sdk::clock::SLOT_MS;
use solana_sdk::timing::AtomicInterval;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
pub type Age = u8;

pub const AGE_MS: u64 = SLOT_MS; // match one age per slot time

pub struct BucketMapHolder<T: IndexValue> {
    pub disk: Option<BucketMap<SlotT<T>>>,

    pub count_ages_flushed: AtomicUsize,
    pub age: AtomicU8,
    pub stats: BucketMapHolderStats,

    age_timer: AtomicInterval,

    // used by bg processing to know when any bucket has become dirty
    pub wait_dirty_or_aged: WaitableCondvar,
    next_bucket_to_flush: Mutex<usize>,
    bins: usize,

    // thread throttling
    throughput_interval: AtomicInterval,
    count_bucket_scans_complete: AtomicUsize,
    pub wait_thread_throttling: WaitableCondvar,
    pub desired_threads: AtomicUsize,
    pub active_threads: AtomicUsize,
    threads: usize,

    // how much mb are we allowed to keep in the in-mem index?
    // Rest goes to disk.
    pub mem_budget_mb: Option<usize>,
    ages_to_stay_in_cache: Age,

    /// startup is a special time for flush to focus on moving everything to disk as fast and efficiently as possible
    /// with less thread count limitations. LRU and access patterns are not important. Freeing memory
    /// and writing to disk in parallel are.
    /// Note startup is an optimization and is not required for correctness.
    startup: AtomicBool,
}

impl<T: IndexValue> Debug for BucketMapHolder<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

#[allow(clippy::mutex_atomic)]
impl<T: IndexValue> BucketMapHolder<T> {
    pub fn increment_age(&self) {
        // since we are about to change age, there are now 0 buckets that have been flushed at this age
        // this should happen before the age.fetch_add
        let previous = self.count_ages_flushed.swap(0, Ordering::Acquire);
        // fetch_add is defined to wrap.
        // That's what we want. 0..255, then back to 0.
        self.age.fetch_add(1, Ordering::Release);
        assert!(previous >= self.bins); // we should not have increased age before previous age was fully flushed
        self.wait_dirty_or_aged.notify_all(); // notify all because we can age scan in parallel
    }

    pub fn future_age_to_flush(&self) -> Age {
        self.current_age().wrapping_add(self.ages_to_stay_in_cache)
    }

    fn has_age_interval_elapsed(&self) -> bool {
        // note that when this returns true, state of age_timer is modified
        self.age_timer.should_update(AGE_MS)
    }

    /// used by bg processes to determine # active threads and how aggressively to flush
    pub fn get_startup(&self) -> bool {
        self.startup.load(Ordering::Relaxed)
    }

    pub fn set_startup(&self, value: bool) {
        if !value {
            self.wait_for_idle();
        }
        self.startup.store(value, Ordering::Relaxed)
    }

    pub(crate) fn wait_for_idle(&self) {
        assert!(self.get_startup());
    }

    pub fn current_age(&self) -> Age {
        self.age.load(Ordering::Acquire)
    }

    pub fn bucket_flushed_at_current_age(&self) {
        self.count_ages_flushed.fetch_add(1, Ordering::Release);
        self.maybe_advance_age();
    }

    pub fn bucket_scan_complete(&self) {
        self.count_bucket_scans_complete
            .fetch_add(1, Ordering::Acquire);
    }

    // have all buckets been flushed at the current age?
    pub fn all_buckets_flushed_at_current_age(&self) -> bool {
        self.count_ages_flushed() >= self.bins
    }

    pub fn count_ages_flushed(&self) -> usize {
        self.count_ages_flushed.load(Ordering::Acquire)
    }

    pub fn maybe_advance_age(&self) -> bool {
        // check has_age_interval_elapsed last as calling it modifies state on success
        if self.all_buckets_flushed_at_current_age() && self.has_age_interval_elapsed() {
            self.increment_age();
            true
        } else {
            false
        }
    }

    pub fn new(bins: usize, config: &Option<AccountsIndexConfig>, threads: usize) -> Self {
        const DEFAULT_AGE_TO_STAY_IN_CACHE: Age = 5;
        let ages_to_stay_in_cache = config
            .as_ref()
            .and_then(|config| config.ages_to_stay_in_cache)
            .unwrap_or(DEFAULT_AGE_TO_STAY_IN_CACHE);

        let mut bucket_config = BucketMapConfig::new(bins);
        bucket_config.drives = config.as_ref().and_then(|config| config.drives.clone());
        let mem_budget_mb = config.as_ref().and_then(|config| config.index_limit_mb);
        // only allocate if mem_budget_mb is Some
        let disk = mem_budget_mb.map(|_| BucketMap::new(bucket_config));
        const INITIAL_DESIRED_THREADS: usize = 1;
        Self {
            disk,
            ages_to_stay_in_cache,
            count_ages_flushed: AtomicUsize::default(),
            count_bucket_scans_complete: AtomicUsize::default(),
            age: AtomicU8::default(),
            stats: BucketMapHolderStats::new(bins),
            wait_dirty_or_aged: WaitableCondvar::default(),
            next_bucket_to_flush: Mutex::new(0),
            age_timer: AtomicInterval::default(),
            bins,
            startup: AtomicBool::default(),
            mem_budget_mb,
            wait_thread_throttling: WaitableCondvar::default(),
            desired_threads: AtomicUsize::new(INITIAL_DESIRED_THREADS),
            active_threads: AtomicUsize::default(),
            throughput_interval: AtomicInterval::default(),
            threads,
        }
    }

    // calculate whether we need to add or reduce # threads
    pub fn evaluate_thread_throttling(&self) {
        const MS_PER_S: u64 = 1000;
        let desired_throughput_bins_per_s = (self.bins as u64) * MS_PER_S / AGE_MS;
        const THROUGHTPUT_INTERVAL_MS: u64 = 100;
        if self
            .throughput_interval
            .should_update(THROUGHTPUT_INTERVAL_MS)
        {
            // time to determine whether to increase or decrease desired threads
            let elapsed_ms = THROUGHTPUT_INTERVAL_MS; // this is an approximation, real value is >= this
            let bins_scanned = self.count_bucket_scans_complete.swap(0, Ordering::Relaxed) as u64;
            let progress = bins_scanned * MS_PER_S / elapsed_ms;
            let slop = desired_throughput_bins_per_s / 2;
            if progress < desired_throughput_bins_per_s - slop {
                // increment desired threads
                let desired = self.desired_threads.load(Ordering::Relaxed);
                if desired < self.threads
                    && self
                        .desired_threads
                        .compare_exchange(
                            desired,
                            desired + 1,
                            Ordering::Acquire,
                            Ordering::Relaxed,
                        )
                        .is_ok()
                {
                    self.wait_thread_throttling.notify_one();
                }
            } else if progress > desired_throughput_bins_per_s + slop {
                // decrement desired threads
                let desired = self.desired_threads.load(Ordering::Relaxed);
                if desired > 1 {
                    // after updating this, an active thread will figure out it needs to go to sleep
                    let _ = self.desired_threads.compare_exchange(
                        desired,
                        desired - 1,
                        Ordering::Acquire,
                        Ordering::Relaxed,
                    );
                }
            }
        }
    }

    // return true if this thread should go to sleep by calling throttle_thread
    pub fn should_throttle_thread(&self) -> bool {
        let desired = self.desired_threads.load(Ordering::Relaxed);
        let active = self.active_threads.load(Ordering::Relaxed);
        if active > desired
            && self
                .active_threads
                .compare_exchange(active, active - 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        {
            return true; // this thread went to sleep to satisfy 'desired'
        }
        false
    }

    // returns when this thread should become active, otherwise wait
    pub fn throttle_thread(&self) {
        loop {
            let desired = self.desired_threads.load(Ordering::Relaxed);
            loop {
                let active = self.active_threads.load(Ordering::Relaxed);
                if active >= desired {
                    break;
                }
                if self
                    .active_threads
                    .compare_exchange(active, active + 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return; // this thread became active to satisfy 'desired'
                }
            }

            // otherwise, this thread should sleep
            if !self
                .wait_thread_throttling
                .wait_timeout(Duration::from_millis(1000))
            {
                break; // wait was triggered, so return
            }
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

    // intended to execute in a bg thread
    pub fn background(&self, exit: Arc<AtomicBool>, in_mem: Vec<Arc<InMemAccountsIndex<T>>>) {
        let bins = in_mem.len();
        let flush = self.disk.is_some();
        loop {
            if self.should_throttle_thread() {
                self.stats.active_threads.fetch_sub(1, Ordering::Relaxed);
                self.throttle_thread();
                self.stats.active_threads.fetch_add(1, Ordering::Relaxed);
            } else {
                self.wait_dirty_or_aged
                    .wait_timeout(Duration::from_millis(AGE_MS));
            }
            if exit.load(Ordering::Relaxed) {
                break;
            }

            self.stats.active_threads.fetch_add(1, Ordering::Relaxed);
            for _ in 0..bins {
                if flush {
                    let index = self.next_bucket_to_flush();
                    in_mem[index].flush();
                    self.evaluate_thread_throttling();
                }
                self.stats.report_stats(self);
            }
            self.stats.active_threads.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    #[test]
    fn test_next_bucket_to_flush() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
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
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        for age in 0..513 {
            assert_eq!(test.current_age(), (age % 256) as Age);

            // inc all
            for _ in 0..bins {
                assert!(!test.all_buckets_flushed_at_current_age());
                // cannot call this because based on timing, it may fire: test.bucket_flushed_at_current_age();
            }

            // this would normally happen once time went off and all buckets had been flushed at the previous age
            test.count_ages_flushed.fetch_add(bins, Ordering::Release);
            test.increment_age();
        }
    }

    #[test]
    fn test_age_time() {
        solana_logger::setup();
        let bins = 1;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        let threads = 2;
        let time = AGE_MS * 5 / 2;
        let expected = (time / AGE_MS) as Age;
        let now = Instant::now();
        test.bucket_flushed_at_current_age(); // done with age 0
        (0..threads).into_par_iter().for_each(|_| {
            while now.elapsed().as_millis() < (time as u128) {
                if test.maybe_advance_age() {
                    test.bucket_flushed_at_current_age();
                }
            }
        });
        assert_eq!(test.current_age(), expected);
    }

    #[test]
    fn test_age_broad() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        assert_eq!(test.current_age(), 0);
        for _ in 0..bins {
            assert!(!test.all_buckets_flushed_at_current_age());
            test.bucket_flushed_at_current_age();
        }
        std::thread::sleep(std::time::Duration::from_millis(AGE_MS * 2));
        test.maybe_advance_age();
        assert_eq!(test.current_age(), 1);
        assert!(!test.all_buckets_flushed_at_current_age());
    }
}
