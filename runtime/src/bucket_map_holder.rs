use {
    crate::{
        accounts_index::{AccountsIndexConfig, IndexLimitMb, IndexValue},
        bucket_map_holder_stats::BucketMapHolderStats,
        in_mem_accounts_index::InMemAccountsIndex,
        waitable_condvar::WaitableCondvar,
    },
    solana_bucket_map::bucket_map::{BucketMap, BucketMapConfig},
    solana_measure::measure::Measure,
    solana_sdk::{
        clock::{Slot, DEFAULT_MS_PER_SLOT},
        timing::AtomicInterval,
    },
    std::{
        fmt::Debug,
        sync::{
            atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    },
};
pub type Age = u8;

const AGE_MS: u64 = DEFAULT_MS_PER_SLOT; // match one age per slot time

// 10 GB limit for in-mem idx. In practice, we don't get this high. This tunes how aggressively to save items we expect to use soon.
pub const DEFAULT_DISK_INDEX: Option<usize> = Some(10_000);

pub struct BucketMapHolder<T: IndexValue> {
    pub disk: Option<BucketMap<(Slot, T)>>,

    pub count_buckets_flushed: AtomicUsize,

    /// These three ages are individual atomics because their values are read many times from code during runtime.
    /// Instead of accessing the single age and doing math each time, each value is incremented each time the age occurs, which is ~400ms.
    /// Callers can ask for the precomputed value they already want.
    /// rolling 'current' age
    pub age: AtomicU8,
    /// rolling age that is 'ages_to_stay_in_cache' + 'age'
    pub future_age_to_flush: AtomicU8,
    /// rolling age that is effectively 'age' - 1
    /// these items are expected to be flushed from the accounts write cache or otherwise modified before this age occurs
    pub future_age_to_flush_cached: AtomicU8,

    pub stats: BucketMapHolderStats,

    age_timer: AtomicInterval,

    // used by bg processing to know when any bucket has become dirty
    pub wait_dirty_or_aged: Arc<WaitableCondvar>,
    next_bucket_to_flush: AtomicUsize,
    bins: usize,

    pub threads: usize,

    // how much mb are we allowed to keep in the in-mem index?
    // Rest goes to disk.
    pub mem_budget_mb: Option<usize>,

    /// how many ages should elapse from the last time an item is used where the item will remain in the cache
    pub ages_to_stay_in_cache: Age,

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
    /// is the accounts index using disk as a backing store
    pub fn is_disk_index_enabled(&self) -> bool {
        self.disk.is_some()
    }

    pub fn increment_age(&self) {
        // since we are about to change age, there are now 0 buckets that have been flushed at this age
        // this should happen before the age.fetch_add
        // Otherwise, as soon as we increment the age, a thread could race us and flush before we swap this out since it detects the age has moved forward and a bucket will be eligible for flushing.
        let previous = self.count_buckets_flushed.swap(0, Ordering::AcqRel);
        // fetch_add is defined to wrap.
        // That's what we want. 0..255, then back to 0.
        self.age.fetch_add(1, Ordering::Release);
        self.future_age_to_flush.fetch_add(1, Ordering::Release);
        self.future_age_to_flush_cached
            .fetch_add(1, Ordering::Release);
        assert!(
            previous >= self.bins,
            "previous: {}, bins: {}",
            previous,
            self.bins
        ); // we should not have increased age before previous age was fully flushed
        self.wait_dirty_or_aged.notify_all(); // notify all because we can age scan in parallel
    }

    pub fn future_age_to_flush(&self, is_cached: bool) -> Age {
        if is_cached {
            &self.future_age_to_flush_cached
        } else {
            &self.future_age_to_flush
        }
        .load(Ordering::Acquire)
    }

    fn has_age_interval_elapsed(&self) -> bool {
        // note that when this returns true, state of age_timer is modified
        self.age_timer.should_update(self.age_interval_ms())
    }

    /// used by bg processes to determine # active threads and how aggressively to flush
    pub fn get_startup(&self) -> bool {
        self.startup.load(Ordering::Relaxed)
    }

    /// startup=true causes:
    ///      in mem to act in a way that flushes to disk asap
    /// startup=false is 'normal' operation
    pub fn set_startup(&self, value: bool) {
        if !value {
            self.wait_for_idle();
        }
        self.startup.store(value, Ordering::Relaxed)
    }

    /// return when the bg threads have reached an 'idle' state
    pub(crate) fn wait_for_idle(&self) {
        assert!(self.get_startup());
        if self.disk.is_none() {
            return;
        }

        // when age has incremented twice, we know that we have made it through scanning all bins since we started waiting,
        //  so we are then 'idle'
        let end_age = self.current_age().wrapping_add(2);
        loop {
            self.wait_dirty_or_aged
                .wait_timeout(Duration::from_millis(self.age_interval_ms()));
            if end_age == self.current_age() {
                break;
            }
        }
    }

    pub fn current_age(&self) -> Age {
        self.age.load(Ordering::Acquire)
    }

    pub fn bucket_flushed_at_current_age(&self, can_advance_age: bool) {
        let count_buckets_flushed = 1 + self.count_buckets_flushed.fetch_add(1, Ordering::AcqRel);
        if can_advance_age {
            self.maybe_advance_age_internal(
                self.all_buckets_flushed_at_current_age_internal(count_buckets_flushed),
            );
        }
    }

    /// have all buckets been flushed at the current age?
    pub fn all_buckets_flushed_at_current_age(&self) -> bool {
        self.all_buckets_flushed_at_current_age_internal(self.count_buckets_flushed())
    }

    /// have all buckets been flushed at the current age?
    fn all_buckets_flushed_at_current_age_internal(&self, count_buckets_flushed: usize) -> bool {
        count_buckets_flushed >= self.bins
    }

    pub fn count_buckets_flushed(&self) -> usize {
        self.count_buckets_flushed.load(Ordering::Acquire)
    }

    /// if all buckets are flushed at the current age and time has elapsed, then advance age
    pub fn maybe_advance_age(&self) -> bool {
        self.maybe_advance_age_internal(self.all_buckets_flushed_at_current_age())
    }

    /// if all buckets are flushed at the current age and time has elapsed, then advance age
    fn maybe_advance_age_internal(&self, all_buckets_flushed_at_current_age: bool) -> bool {
        // call has_age_interval_elapsed last since calling it modifies state on success
        if all_buckets_flushed_at_current_age && self.has_age_interval_elapsed() {
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
        let mem_budget_mb = match config
            .as_ref()
            .map(|config| &config.index_limit_mb)
            .unwrap_or(&IndexLimitMb::Unspecified)
        {
            // creator said to use disk idx with a specific limit
            IndexLimitMb::Limit(mb) => Some(*mb),
            // creator said InMemOnly, so no disk index
            IndexLimitMb::InMemOnly => None,
            // whatever started us didn't specify whether to use the acct idx
            IndexLimitMb::Unspecified => {
                // check env var if we were not started from a validator
                let mut use_default = true;
                if !config
                    .as_ref()
                    .map(|config| config.started_from_validator)
                    .unwrap_or_default()
                {
                    if let Ok(_limit) = std::env::var("SOLANA_TEST_ACCOUNTS_INDEX_MEMORY_LIMIT_MB")
                    {
                        // Note this env var means the opposite of the default. The default now is disk index is on.
                        // So, if this env var is set, DO NOT allocate with disk buckets if mem budget was not set, we were NOT started from validator, and env var was set
                        // we do not want the env var to have an effect when running the validator (only tests, benches, etc.)
                        use_default = false;
                    }
                }
                if use_default {
                    // if validator does not specify disk index limit or specify in mem only, then this is the default
                    DEFAULT_DISK_INDEX
                } else {
                    None
                }
            }
        };

        // only allocate if mem_budget_mb is Some
        let disk = mem_budget_mb.map(|_| BucketMap::new(bucket_config));
        Self {
            disk,
            ages_to_stay_in_cache,
            count_buckets_flushed: AtomicUsize::default(),
            // age = 0
            age: AtomicU8::default(),
            // future age = age (=0) + ages_to_stay_in_cache
            future_age_to_flush: AtomicU8::new(ages_to_stay_in_cache),
            // effectively age (0) - 1. So, the oldest possible age from 'now'
            future_age_to_flush_cached: AtomicU8::new(0_u8.wrapping_sub(1)),
            stats: BucketMapHolderStats::new(bins),
            wait_dirty_or_aged: Arc::default(),
            next_bucket_to_flush: AtomicUsize::new(0),
            age_timer: AtomicInterval::default(),
            bins,
            startup: AtomicBool::default(),
            mem_budget_mb,
            threads,
        }
    }

    // get the next bucket to flush, with the idea that the previous bucket
    // is perhaps being flushed by another thread already.
    pub fn next_bucket_to_flush(&self) -> usize {
        self.next_bucket_to_flush
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bucket| {
                Some((bucket + 1) % self.bins)
            })
            .unwrap()
    }

    /// prepare for this to be dynamic if necessary
    /// For example, maybe startup has a shorter age interval.
    fn age_interval_ms(&self) -> u64 {
        AGE_MS
    }

    /// return an amount of ms to sleep
    fn throttling_wait_ms_internal(
        &self,
        interval_ms: u64,
        elapsed_ms: u64,
        bins_flushed: u64,
    ) -> Option<u64> {
        let target_percent = 90; // aim to finish in 90% of the allocated time
        let remaining_ms = (interval_ms * target_percent / 100).saturating_sub(elapsed_ms);
        let remaining_bins = (self.bins as u64).saturating_sub(bins_flushed);
        if remaining_bins == 0 || remaining_ms == 0 || elapsed_ms == 0 || bins_flushed == 0 {
            // any of these conditions result in 'do not wait due to progress'
            return None;
        }
        let ms_per_s = 1_000;
        let rate_bins_per_s = bins_flushed * ms_per_s / elapsed_ms;
        let expected_bins_processed_in_remaining_time = rate_bins_per_s * remaining_ms / ms_per_s;
        if expected_bins_processed_in_remaining_time > remaining_bins {
            // wait because we predict will finish prior to target
            Some(1)
        } else {
            // do not wait because we predict will finish after target
            None
        }
    }

    /// Check progress this age.
    /// Return ms to wait to get closer to the wait target and spread out work over the entire age interval.
    /// Goal is to avoid cpu spikes at beginning of age interval.
    fn throttling_wait_ms(&self) -> Option<u64> {
        let interval_ms = self.age_interval_ms();
        let elapsed_ms = self.age_timer.elapsed_ms();
        let bins_flushed = self.count_buckets_flushed() as u64;
        self.throttling_wait_ms_internal(interval_ms, elapsed_ms, bins_flushed)
    }

    /// true if this thread can sleep
    fn should_thread_sleep(&self) -> bool {
        let bins_flushed = self.count_buckets_flushed();
        if bins_flushed >= self.bins {
            // all bins flushed, so this thread can sleep
            true
        } else {
            // at least 1 thread running for each bin that still needs to be flushed, so this thread can sleep
            let active = self.stats.active_threads.load(Ordering::Relaxed);
            bins_flushed.saturating_add(active as usize) >= self.bins
        }
    }

    // intended to execute in a bg thread
    pub fn background(
        &self,
        exit: Vec<Arc<AtomicBool>>,
        in_mem: Vec<Arc<InMemAccountsIndex<T>>>,
        can_advance_age: bool,
    ) {
        let bins = in_mem.len();
        let flush = self.disk.is_some();
        let mut throttling_wait_ms = None;
        loop {
            if !flush {
                self.wait_dirty_or_aged.wait_timeout(Duration::from_millis(
                    self.stats.remaining_until_next_interval(),
                ));
            } else if self.should_thread_sleep() || throttling_wait_ms.is_some() {
                let mut wait = std::cmp::min(
                    self.age_timer
                        .remaining_until_next_interval(self.age_interval_ms()),
                    self.stats.remaining_until_next_interval(),
                );
                if !can_advance_age {
                    // if this thread cannot advance age, then make sure we don't sleep 0
                    wait = wait.max(1);
                }
                if let Some(throttling_wait_ms) = throttling_wait_ms {
                    self.stats
                        .bg_throttling_wait_us
                        .fetch_add(throttling_wait_ms * 1000, Ordering::Relaxed);
                    wait = std::cmp::min(throttling_wait_ms, wait);
                }

                let mut m = Measure::start("wait");
                self.wait_dirty_or_aged
                    .wait_timeout(Duration::from_millis(wait));
                m.stop();
                self.stats
                    .bg_waiting_us
                    .fetch_add(m.as_us(), Ordering::Relaxed);
                // likely some time has elapsed. May have been waiting for age time interval to elapse.
                if can_advance_age {
                    self.maybe_advance_age();
                }
            }
            throttling_wait_ms = None;

            if exit.iter().any(|exit| exit.load(Ordering::Relaxed)) {
                break;
            }

            self.stats.active_threads.fetch_add(1, Ordering::Relaxed);
            for _ in 0..bins {
                if flush {
                    let index = self.next_bucket_to_flush();
                    in_mem[index].flush(can_advance_age);
                }
                self.stats.report_stats(self);
                if self.all_buckets_flushed_at_current_age() {
                    break;
                }
                throttling_wait_ms = self.throttling_wait_ms();
                if throttling_wait_ms.is_some() {
                    break;
                }
            }
            self.stats.active_threads.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {super::*, rayon::prelude::*, std::time::Instant};

    #[test]
    fn test_next_bucket_to_flush() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        let visited = (0..bins)
            .map(|_| AtomicUsize::default())
            .collect::<Vec<_>>();
        let iterations = bins * 30;
        let threads = bins * 4;
        let expected = threads * iterations / bins;

        (0..threads).into_par_iter().for_each(|_| {
            (0..iterations).for_each(|_| {
                let bin = test.next_bucket_to_flush();
                visited[bin].fetch_add(1, Ordering::Relaxed);
            });
        });
        visited.iter().enumerate().for_each(|(bin, visited)| {
            assert_eq!(visited.load(Ordering::Relaxed), expected, "bin: {bin}")
        });
    }

    #[test]
    fn test_ages() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        assert_eq!(0, test.current_age());
        assert_eq!(test.ages_to_stay_in_cache, test.future_age_to_flush(false));
        assert_eq!(u8::MAX, test.future_age_to_flush(true));
        (0..bins).for_each(|_| {
            test.bucket_flushed_at_current_age(false);
        });
        test.increment_age();
        assert_eq!(1, test.current_age());
        assert_eq!(
            test.ages_to_stay_in_cache + 1,
            test.future_age_to_flush(false)
        );
        assert_eq!(0, test.future_age_to_flush(true));
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
            test.count_buckets_flushed
                .fetch_add(bins, Ordering::Release);
            test.increment_age();
        }
    }

    #[test]
    fn test_throttle() {
        solana_logger::setup();
        let bins = 128;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        let bins = test.bins as u64;
        let interval_ms = test.age_interval_ms();
        // 90% of time elapsed, all but 1 bins flushed, should not wait since we'll end up right on time
        let elapsed_ms = interval_ms * 89 / 100;
        let bins_flushed = bins - 1;
        let result = test.throttling_wait_ms_internal(interval_ms, elapsed_ms, bins_flushed);
        assert_eq!(result, None);
        // 10% of time, all bins but 1, should wait
        let elapsed_ms = interval_ms / 10;
        let bins_flushed = bins - 1;
        let result = test.throttling_wait_ms_internal(interval_ms, elapsed_ms, bins_flushed);
        assert_eq!(result, Some(1));
        // 5% of time, 8% of bins, should wait. target is 90%. These #s roughly work
        let elapsed_ms = interval_ms * 5 / 100;
        let bins_flushed = bins * 8 / 100;
        let result = test.throttling_wait_ms_internal(interval_ms, elapsed_ms, bins_flushed);
        assert_eq!(result, Some(1));
        // 11% of time, 12% of bins, should NOT wait. target is 90%. These #s roughly work
        let elapsed_ms = interval_ms * 11 / 100;
        let bins_flushed = bins * 12 / 100;
        let result = test.throttling_wait_ms_internal(interval_ms, elapsed_ms, bins_flushed);
        assert_eq!(result, None);
    }

    #[test]
    fn test_disk_index_enabled() {
        let bins = 1;
        let config = AccountsIndexConfig {
            index_limit_mb: IndexLimitMb::Limit(0),
            ..AccountsIndexConfig::default()
        };
        let test = BucketMapHolder::<u64>::new(bins, &Some(config), 1);
        assert!(test.is_disk_index_enabled());
    }

    #[test]
    fn test_age_time() {
        solana_logger::setup();
        let bins = 1;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        let threads = 2;
        let time = AGE_MS * 8 / 3;
        let expected = (time / AGE_MS) as Age;
        let now = Instant::now();
        test.bucket_flushed_at_current_age(true); // done with age 0
        (0..threads).into_par_iter().for_each(|_| {
            // This test used to be more strict with time, but in a parallel, multi test environment,
            // sometimes threads starve and this test intermittently fails. So, give it more time than it should require.
            // This may be aggrevated by the strategy of only allowing thread 0 to advance the age.
            while now.elapsed().as_millis() < (time as u128) * 100 {
                if test.maybe_advance_age() {
                    test.bucket_flushed_at_current_age(true);
                }

                if test.current_age() >= expected {
                    break;
                }
            }
        });
        assert!(
            test.current_age() >= expected,
            "{}, {}",
            test.current_age(),
            expected
        );
    }

    #[test]
    fn test_age_broad() {
        solana_logger::setup();
        let bins = 4;
        let test = BucketMapHolder::<u64>::new(bins, &Some(AccountsIndexConfig::default()), 1);
        assert_eq!(test.current_age(), 0);
        for _ in 0..bins {
            assert!(!test.all_buckets_flushed_at_current_age());
            test.bucket_flushed_at_current_age(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(AGE_MS * 2));
        test.maybe_advance_age();
        assert_eq!(test.current_age(), 1);
        assert!(!test.all_buckets_flushed_at_current_age());
    }
}
