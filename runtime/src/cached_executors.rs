#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
use {
    log::*,
    rand::Rng,
    solana_program_runtime::invoke_context::Executor,
    solana_sdk::{
        clock::{Epoch, Slot},
        pubkey::Pubkey,
        saturating_add_assign,
    },
    std::{
        collections::HashMap,
        ops::Div,
        sync::{
            atomic::{AtomicU64, Ordering::Relaxed},
            Arc,
        },
    },
};

// 10 MB assuming programs are around 100k
pub(crate) const MAX_CACHED_EXECUTORS: usize = 100;

/// LFU Cache of executors with single-epoch memory of usage counts
#[derive(Debug)]
pub(crate) struct CachedExecutors {
    max: usize,
    current_epoch: Epoch,
    executors: HashMap<Pubkey, CachedExecutorsEntry>,
    stats: CachedExecutorsStats,
}

#[derive(Debug)]
struct CachedExecutorsEntry {
    prev_epoch_count: u64,
    epoch_count: AtomicU64,
    executor: Arc<dyn Executor>,
}

#[derive(Debug, Default)]
struct CachedExecutorsStats {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: HashMap<Pubkey, u64>,
    insertions: AtomicU64,
    replacements: AtomicU64,
}

impl Default for CachedExecutors {
    fn default() -> Self {
        Self {
            max: MAX_CACHED_EXECUTORS,
            current_epoch: Epoch::default(),
            executors: HashMap::default(),
            stats: CachedExecutorsStats::default(),
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for CachedExecutors {
    fn example() -> Self {
        // Delegate AbiExample impl to Default before going deep and stuck with
        // not easily impl-able Arc<dyn Executor> due to rust's coherence issue
        // This is safe because CachedExecutors isn't serializable by definition.
        Self::default()
    }
}

impl Clone for CachedExecutors {
    fn clone(&self) -> Self {
        let executors = self.executors.iter().map(|(&key, entry)| {
            let entry = CachedExecutorsEntry {
                prev_epoch_count: entry.prev_epoch_count,
                epoch_count: AtomicU64::new(entry.epoch_count.load(Relaxed)),
                executor: entry.executor.clone(),
            };
            (key, entry)
        });
        Self {
            max: self.max,
            current_epoch: self.current_epoch,
            executors: executors.collect(),
            stats: CachedExecutorsStats::default(),
        }
    }
}

impl CachedExecutors {
    pub(crate) fn clone_with_epoch(self: &Arc<Self>, epoch: Epoch) -> Arc<Self> {
        if self.current_epoch == epoch {
            return self.clone();
        }
        let executors = self.executors.iter().map(|(&key, entry)| {
            // The total_count = prev_epoch_count + epoch_count will be used for LFU eviction.
            // If the epoch has changed, we store the prev_epoch_count and reset the epoch_count to 0.
            let entry = CachedExecutorsEntry {
                prev_epoch_count: entry.epoch_count.load(Relaxed),
                epoch_count: AtomicU64::default(),
                executor: entry.executor.clone(),
            };
            (key, entry)
        });
        Arc::new(Self {
            max: self.max,
            current_epoch: epoch,
            executors: executors.collect(),
            stats: CachedExecutorsStats::default(),
        })
    }

    pub(crate) fn new(max: usize, current_epoch: Epoch) -> Self {
        Self {
            max,
            current_epoch,
            executors: HashMap::new(),
            stats: CachedExecutorsStats::default(),
        }
    }

    pub(crate) fn get(&self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        if let Some(entry) = self.executors.get(pubkey) {
            self.stats.hits.fetch_add(1, Relaxed);
            entry.epoch_count.fetch_add(1, Relaxed);
            Some(entry.executor.clone())
        } else {
            self.stats.misses.fetch_add(1, Relaxed);
            None
        }
    }

    pub(crate) fn put(&mut self, executors: &[(&Pubkey, Arc<dyn Executor>)]) {
        let mut new_executors: Vec<_> = executors
            .iter()
            .filter_map(|(key, executor)| {
                if let Some(mut entry) = self.executors.remove(key) {
                    self.stats.replacements.fetch_add(1, Relaxed);
                    entry.executor = executor.clone();
                    let _ = self.executors.insert(**key, entry);
                    None
                } else {
                    self.stats.insertions.fetch_add(1, Relaxed);
                    Some((*key, executor))
                }
            })
            .collect();

        if !new_executors.is_empty() {
            let mut counts = self
                .executors
                .iter()
                .map(|(key, entry)| {
                    let count = entry.prev_epoch_count + entry.epoch_count.load(Relaxed);
                    (key, count)
                })
                .collect::<Vec<_>>();
            counts.sort_unstable_by_key(|(_, count)| *count);

            let primer_counts = Self::get_primer_counts(counts.as_slice(), new_executors.len());

            if self.executors.len() >= self.max {
                let mut least_keys = counts
                    .iter()
                    .take(new_executors.len())
                    .map(|least| *least.0)
                    .collect::<Vec<_>>();
                for least_key in least_keys.drain(..) {
                    let _ = self.executors.remove(&least_key);
                    self.stats
                        .evictions
                        .entry(least_key)
                        .and_modify(|c| saturating_add_assign!(*c, 1))
                        .or_insert(1);
                }
            }

            for ((key, executor), primer_count) in new_executors.drain(..).zip(primer_counts) {
                let entry = CachedExecutorsEntry {
                    prev_epoch_count: 0,
                    epoch_count: AtomicU64::new(primer_count),
                    executor: executor.clone(),
                };
                let _ = self.executors.insert(*key, entry);
            }
        }
    }

    pub(crate) fn remove(&mut self, pubkey: &Pubkey) {
        self.executors.remove(pubkey);
    }

    fn get_primer_count_upper_bound_inclusive(counts: &[(&Pubkey, u64)]) -> u64 {
        const PRIMER_COUNT_TARGET_PERCENTILE: u64 = 85;
        #[allow(clippy::assertions_on_constants)]
        {
            assert!(PRIMER_COUNT_TARGET_PERCENTILE <= 100);
        }
        // Executor use-frequencies are assumed to fit a Pareto distribution.  Choose an
        // upper-bound for our primer count as the actual count at the target rank to avoid
        // an upward bias

        let target_index = u64::try_from(counts.len().saturating_sub(1))
            .ok()
            .and_then(|counts| {
                let index = counts
                    .saturating_mul(PRIMER_COUNT_TARGET_PERCENTILE)
                    .div(100); // switch to u64::saturating_div once stable
                usize::try_from(index).ok()
            })
            .unwrap_or(0);

        counts
            .get(target_index)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    }

    fn get_primer_counts(counts: &[(&Pubkey, u64)], num_counts: usize) -> Vec<u64> {
        let max_primer_count = Self::get_primer_count_upper_bound_inclusive(counts);
        let mut rng = rand::thread_rng();

        (0..num_counts)
            .map(|_| rng.gen_range(0, max_primer_count.saturating_add(1)))
            .collect::<Vec<_>>()
    }

    pub(crate) fn submit_stats(&self, slot: Slot) {
        self.stats.submit(slot);
    }
}

impl CachedExecutorsStats {
    fn submit(&self, slot: Slot) {
        let hits = self.hits.load(Relaxed);
        let misses = self.misses.load(Relaxed);
        let insertions = self.insertions.load(Relaxed);
        let replacements = self.replacements.load(Relaxed);
        let evictions: u64 = self.evictions.values().sum();
        datapoint_info!(
            "bank-executor-cache-stats",
            ("slot", slot, i64),
            ("hits", hits, i64),
            ("misses", misses, i64),
            ("evictions", evictions, i64),
            ("insertions", insertions, i64),
            ("replacements", replacements, i64),
        );
        debug!(
                "Executor Cache Stats -- Hits: {}, Misses: {}, Evictions: {}, Insertions: {}, Replacements: {}",
                hits, misses, evictions, insertions, replacements,
            );
        if log_enabled!(log::Level::Trace) && !self.evictions.is_empty() {
            let mut evictions = self.evictions.iter().collect::<Vec<_>>();
            evictions.sort_by_key(|e| e.1);
            let evictions = evictions
                .into_iter()
                .rev()
                .map(|(program_id, evictions)| {
                    format!("  {:<44}  {}", program_id.to_string(), evictions)
                })
                .collect::<Vec<_>>();
            let evictions = evictions.join("\n");
            trace!(
                "Eviction Details:\n  {:<44}  {}\n{}",
                "Program",
                "Count",
                evictions
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, solana_program_runtime::invoke_context::InvokeContext,
        solana_sdk::instruction::InstructionError,
    };

    #[derive(Debug)]
    struct TestExecutor {}
    impl Executor for TestExecutor {
        fn execute<'a, 'b>(
            &self,
            _first_instruction_account: usize,
            _instruction_data: &[u8],
            _invoke_context: &'a mut InvokeContext<'b>,
            _use_jit: bool,
        ) -> std::result::Result<(), InstructionError> {
            Ok(())
        }
    }

    #[test]
    fn test_cached_executors() {
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let key3 = solana_sdk::pubkey::new_rand();
        let key4 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});
        let mut cache = CachedExecutors::new(3, 0);

        cache.put(&[(&key1, executor.clone())]);
        cache.put(&[(&key2, executor.clone())]);
        cache.put(&[(&key3, executor.clone())]);
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_some());

        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());
        cache.put(&[(&key4, executor.clone())]);
        assert!(cache.get(&key4).is_some());
        let num_retained = [&key1, &key2, &key3]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 2);

        assert!(cache.get(&key4).is_some());
        assert!(cache.get(&key4).is_some());
        assert!(cache.get(&key4).is_some());
        cache.put(&[(&key3, executor.clone())]);
        assert!(cache.get(&key3).is_some());
        let num_retained = [&key1, &key2, &key4]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 2);
    }

    #[test]
    fn test_cached_executor_eviction() {
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let key3 = solana_sdk::pubkey::new_rand();
        let key4 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});
        let mut cache = CachedExecutors::new(3, 0);
        assert!(cache.current_epoch == 0);

        cache.put(&[(&key1, executor.clone())]);
        cache.put(&[(&key2, executor.clone())]);
        cache.put(&[(&key3, executor.clone())]);
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key1).is_some());

        let mut cache = Arc::new(cache).clone_with_epoch(1);
        assert!(cache.current_epoch == 1);

        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_some());
        Arc::make_mut(&mut cache).put(&[(&key4, executor.clone())]);

        assert!(cache.get(&key4).is_some());
        let num_retained = [&key1, &key2, &key3]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 2);

        Arc::make_mut(&mut cache).put(&[(&key1, executor.clone())]);
        Arc::make_mut(&mut cache).put(&[(&key3, executor.clone())]);
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key3).is_some());
        let num_retained = [&key2, &key4]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 1);

        cache = cache.clone_with_epoch(2);
        assert!(cache.current_epoch == 2);

        Arc::make_mut(&mut cache).put(&[(&key3, executor.clone())]);
        assert!(cache.get(&key3).is_some());
    }

    #[test]
    fn test_cached_executors_evicts_smallest() {
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let key3 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});
        let mut cache = CachedExecutors::new(2, 0);

        cache.put(&[(&key1, executor.clone())]);
        for _ in 0..5 {
            let _ = cache.get(&key1);
        }
        cache.put(&[(&key2, executor.clone())]);
        // make key1's use-count for sure greater than key2's
        let _ = cache.get(&key1);

        let mut entries = cache
            .executors
            .iter()
            .map(|(k, v)| (*k, v.epoch_count.load(Relaxed)))
            .collect::<Vec<_>>();
        entries.sort_by_key(|(_, v)| *v);
        assert!(entries[0].1 < entries[1].1);

        cache.put(&[(&key3, executor.clone())]);
        assert!(cache.get(&entries[0].0).is_none());
        assert!(cache.get(&entries[1].0).is_some());
    }

    #[test]
    fn test_executor_cache_get_primer_count_upper_bound_inclusive() {
        let pubkey = Pubkey::default();
        let v = [];
        assert_eq!(
            CachedExecutors::get_primer_count_upper_bound_inclusive(&v),
            0
        );
        let v = [(&pubkey, 1)];
        assert_eq!(
            CachedExecutors::get_primer_count_upper_bound_inclusive(&v),
            1
        );
        let v = (0u64..10).map(|i| (&pubkey, i)).collect::<Vec<_>>();
        assert_eq!(
            CachedExecutors::get_primer_count_upper_bound_inclusive(v.as_slice()),
            7
        );
    }
}
