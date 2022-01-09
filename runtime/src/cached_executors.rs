#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
use {
    indexmap::IndexMap,
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
        iter::repeat_with,
        sync::{
            atomic::{AtomicU64, Ordering::Relaxed},
            Arc,
        },
    },
};

// 10 MB assuming programs are around 100k
pub(crate) const MAX_CACHED_EXECUTORS: usize = 100;
// The LFU entry in a random sample of below size is evicted from the cache.
const RANDOM_SAMPLE_SIZE: usize = 2;

/// LFU Cache of executors with single-epoch memory of usage counts
#[derive(Debug)]
pub(crate) struct CachedExecutors {
    max: usize,
    current_epoch: Epoch,
    executors: IndexMap<Pubkey, Arc<dyn Executor>>,
    entries: IndexMap<Pubkey, CachedExecutorsEntry>,
    stats: CachedExecutorsStats,
}

#[derive(Debug, Default)]
struct CachedExecutorsEntry {
    prev_epoch_count: u64,
    epoch_count: AtomicU64,
}

#[derive(Debug, Default)]
struct CachedExecutorsStats {
    hits: AtomicU64,
    num_gets: AtomicU64,
    evictions: HashMap<Pubkey, u64>,
    insertions: AtomicU64,
    replacements: AtomicU64,
}

impl Default for CachedExecutors {
    fn default() -> Self {
        Self {
            max: MAX_CACHED_EXECUTORS,
            current_epoch: Epoch::default(),
            executors: IndexMap::default(),
            entries: IndexMap::default(),
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
        let entries = self.entries.iter().map(|(&key, entry)| {
            let entry = CachedExecutorsEntry {
                prev_epoch_count: entry.prev_epoch_count,
                epoch_count: AtomicU64::new(entry.epoch_count.load(Relaxed)),
            };
            (key, entry)
        });
        Self {
            max: self.max,
            current_epoch: self.current_epoch,
            executors: self.executors.clone(),
            entries: entries.collect(),
            stats: CachedExecutorsStats::default(),
        }
    }
}

impl CachedExecutors {
    pub(crate) fn clone_with_epoch(self: &Arc<Self>, epoch: Epoch) -> Arc<Self> {
        if self.current_epoch == epoch {
            return self.clone();
        }
        let entries = self.entries.iter().map(|(&key, entry)| {
            // The total_count = prev_epoch_count + epoch_count will be used for LFU eviction.
            // If the epoch has changed, we store the prev_epoch_count and reset the epoch_count to 0.
            let entry = CachedExecutorsEntry {
                prev_epoch_count: entry.epoch_count.load(Relaxed),
                epoch_count: AtomicU64::default(),
            };
            (key, entry)
        });
        Arc::new(Self {
            max: self.max,
            current_epoch: epoch,
            executors: self.executors.clone(),
            entries: entries.collect(),
            stats: CachedExecutorsStats::default(),
        })
    }

    pub(crate) fn new(max: usize, current_epoch: Epoch) -> Self {
        Self {
            max,
            current_epoch,
            executors: IndexMap::default(),
            entries: IndexMap::default(),
            stats: CachedExecutorsStats::default(),
        }
    }

    pub(crate) fn get(&self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        self.stats.num_gets.fetch_add(1, Relaxed);
        // self.entries never discards keys with cached executor;
        // So if the key is not in the entries, the executor is not cached.
        let entry = self.entries.get(pubkey)?;
        entry.epoch_count.fetch_add(1, Relaxed);
        let executor = self.executors.get(pubkey)?;
        self.stats.hits.fetch_add(1, Relaxed);
        Some(executor.clone())
    }

    pub(crate) fn put(&mut self, executors: Vec<(Pubkey, Arc<dyn Executor>)>) {
        for (pubkey, executor) in executors {
            self.executors.insert(pubkey, executor);
            self.entries.entry(pubkey).or_default();
        }
        let mut rng = rand::thread_rng();
        // Evict the key with the lowest hits in a random sample of entries.
        while self.executors.len() > self.max {
            let size = self.executors.len();
            let index = repeat_with(|| rng.gen_range(0, size))
                .take(RANDOM_SAMPLE_SIZE)
                .min_by_key(|&index| {
                    let (key, _) = self.executors.get_index(index).unwrap();
                    self.entries[key].num_hits()
                })
                .unwrap();
            let (key, _) = self.executors.swap_remove_index(index).unwrap();
            let count = self.stats.evictions.entry(key).or_default();
            saturating_add_assign!(*count, 1)
        }
        while self.entries.len() > self.max.saturating_mul(20) {
            let size = self.entries.len();
            let index = repeat_with(|| rng.gen_range(0, size))
                .filter(|&index| {
                    let (key, _) = self.entries.get_index(index).unwrap();
                    !self.executors.contains_key(key)
                })
                .take(RANDOM_SAMPLE_SIZE)
                .min_by_key(|&index| self.entries[index].num_hits())
                .unwrap();
            self.entries.swap_remove_index(index);
        }
    }

    pub(crate) fn remove(&mut self, pubkey: &Pubkey) {
        self.executors.remove(pubkey);
    }

    pub(crate) fn submit_stats(&self, slot: Slot) {
        self.stats.submit(slot);
    }
}

impl CachedExecutorsEntry {
    fn num_hits(&self) -> u64 {
        self.epoch_count
            .load(Relaxed)
            .saturating_add(self.prev_epoch_count)
    }
}

impl CachedExecutorsStats {
    fn submit(&self, slot: Slot) {
        let hits = self.hits.load(Relaxed);
        let misses = self.num_gets.load(Relaxed) - hits;
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

        cache.put(vec![(key1, executor.clone())]);
        cache.put(vec![(key2, executor.clone())]);
        cache.put(vec![(key3, executor.clone())]);
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_some());

        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());
        cache.put(vec![(key4, executor.clone())]);
        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 3);

        cache.put(vec![(key3, executor.clone())]);
        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 3);
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

        cache.put(vec![(key1, executor.clone())]);
        cache.put(vec![(key2, executor.clone())]);
        cache.put(vec![(key3, executor.clone())]);
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key1).is_some());

        let mut cache = Arc::new(cache).clone_with_epoch(1);
        assert!(cache.current_epoch == 1);

        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_some());
        Arc::make_mut(&mut cache).put(vec![(key4, executor.clone())]);

        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 3);

        Arc::make_mut(&mut cache).put(vec![(key1, executor.clone())]);
        Arc::make_mut(&mut cache).put(vec![(key3, executor.clone())]);
        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 3);

        cache = cache.clone_with_epoch(2);
        assert!(cache.current_epoch == 2);

        Arc::make_mut(&mut cache).put(vec![(key3, executor.clone())]);
        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .map(|key| cache.get(key))
            .flatten()
            .count();
        assert_eq!(num_retained, 3);
    }
}
