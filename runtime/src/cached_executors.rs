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
    indexmap::{map::Entry,IndexMap,},
    std::{
        collections::HashMap,
        ops::Div,
        iter::repeat_with,
        sync::{
            atomic::{AtomicU64, Ordering::Relaxed},
            Arc,
        },
    },
};

pub(crate) mod executor_cache {
    use {super::*, log};

    #[derive(Debug, Default)]
    pub struct Stats {
        pub hits: AtomicU64,
        pub misses: AtomicU64,
        pub evictions: HashMap<Pubkey, u64>,
        pub insertions: AtomicU64,
        pub replacements: AtomicU64,
        pub one_hit_wonders: AtomicU64,
    }

    impl Stats {
        pub fn submit(&self, slot: Slot) {
            let hits = self.hits.load(Relaxed);
            let misses = self.misses.load(Relaxed);
            let insertions = self.insertions.load(Relaxed);
            let replacements = self.replacements.load(Relaxed);
            let one_hit_wonders = self.one_hit_wonders.load(Relaxed);
            let evictions: u64 = self.evictions.values().sum();
            datapoint_info!(
                "bank-executor-cache-stats",
                ("slot", slot, i64),
                ("hits", hits, i64),
                ("misses", misses, i64),
                ("evictions", evictions, i64),
                ("insertions", insertions, i64),
                ("replacements", replacements, i64),
                ("one_hit_wonders", one_hit_wonders, i64),
            );
            debug!(
                "Executor Cache Stats -- Hits: {}, Misses: {}, Evictions: {}, Insertions: {}, Replacements: {}, One-Hit-Wonders: {}",
                hits, misses, evictions, insertions, replacements, one_hit_wonders,
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
}

pub(crate) const MAX_CACHED_EXECUTORS: usize = 256;

// The LFU entry in a random sample of below size is evicted from the cache.
const RANDOM_SAMPLE_SIZE: usize = 2;

#[derive(Debug)]
pub(crate) struct CachedExecutorsEntry {
    prev_epoch_count: u64,
    epoch_count: AtomicU64,
    executor: Arc<dyn Executor>,
    hit_count: AtomicU64,
}

impl Clone for CachedExecutorsEntry {
    fn clone(&self) -> Self {
        Self {
            prev_epoch_count: self.prev_epoch_count,
            epoch_count: AtomicU64::new(self.epoch_count.load(Relaxed)),
            executor: self.executor.clone(),
            hit_count: AtomicU64::new(self.hit_count.load(Relaxed)),
        }
    }
}

/// LFU Cache of executors with single-epoch memory of usage counts
#[derive(Debug)]
pub(crate) struct CachedExecutors {
    capacity: usize,
    current_epoch: Epoch,
    pub(self) executors: IndexMap<Pubkey, CachedExecutorsEntry>,
    stats: executor_cache::Stats,
}

impl Default for CachedExecutors {
    fn default() -> Self {
        Self {
            capacity: MAX_CACHED_EXECUTORS,
            current_epoch: Epoch::default(),
            executors: IndexMap::new(),
            stats: executor_cache::Stats::default(),
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

impl CachedExecutorsEntry {
    fn num_hits(&self) -> u64 {
        self.epoch_count
            .load(Relaxed)
            .saturating_add(self.prev_epoch_count)
    }
}

impl CachedExecutors {
    pub(crate) fn new(max_capacity: usize, current_epoch: Epoch) -> Self {
        Self {
            capacity: max_capacity,
            current_epoch,
            executors: IndexMap::new(),
            stats: executor_cache::Stats::default(),
        }
    }

    pub(crate) fn new_from_parent_bank_executors(
        parent_bank_executors: &CachedExecutors,
        current_epoch: Epoch,
    ) -> Self {
        let executors = if parent_bank_executors.current_epoch == current_epoch {
            parent_bank_executors.executors.clone()
        } else {
            parent_bank_executors
                .executors
                .iter()
                .map(|(&key, entry)| {
                    let entry = CachedExecutorsEntry {
                        prev_epoch_count: entry.epoch_count.load(Relaxed),
                        epoch_count: AtomicU64::default(),
                        executor: entry.executor.clone(),
                        hit_count: AtomicU64::new(entry.hit_count.load(Relaxed)),
                    };
                    (key, entry)
                })
                .collect()
        };

        Self {
            capacity: parent_bank_executors.capacity,
            current_epoch,
            executors,
            stats: executor_cache::Stats::default(),
        }
    }

    pub(crate) fn get(&self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        if let Some(entry) = self.executors.get(pubkey) {
            self.stats.hits.fetch_add(1, Relaxed);
            entry.epoch_count.fetch_add(1, Relaxed);
            entry.hit_count.fetch_add(1, Relaxed);
            Some(entry.executor.clone())
        } else {
            self.stats.misses.fetch_add(1, Relaxed);
            None
        }
    }

    pub(crate) fn put(&mut self, executors: Vec<(Pubkey, Arc<dyn Executor>)>) {
        for (key, executor) in executors {
            match self.executors.entry(key) {
                Entry::Vacant(entry) => {
                    self.stats.insertions.fetch_add(1, Relaxed);
                    entry.insert(CachedExecutorsEntry {
                        prev_epoch_count: u64::default(),
                        epoch_count: AtomicU64::default(),
                        executor,
                        hit_count: AtomicU64::new(1),
                    });
                },
                Entry::Occupied(mut entry) => {
                    self.stats.replacements.fetch_add(1, Relaxed);
                    entry.get_mut().executor = executor;
                }
            }
        }

        let mut rng = rand::thread_rng();
        // Evict the key with the lowest hits in a random sample of entries.
        while self.executors.len() > self.capacity {
            let size = self.executors.len();
            let index = repeat_with(|| rng.gen_range(0, size))
                .take(RANDOM_SAMPLE_SIZE)
                .min_by_key(|&index| self.executors[index].num_hits())
                .unwrap();
            let (key, _) = self.executors.swap_remove_index(index).unwrap();
            let count = self.stats.evictions.entry(key).or_default();
            saturating_add_assign!(*count, 1)
        }
    }

    pub(crate) fn remove(&mut self, pubkey: &Pubkey) -> Option<CachedExecutorsEntry> {
        let maybe_entry = self.executors.remove(pubkey);
        if let Some(entry) = maybe_entry.as_ref() {
            if entry.hit_count.load(Relaxed) == 1 {
                self.stats.one_hit_wonders.fetch_add(1, Relaxed);
            }
        }
        maybe_entry
    }

    pub(crate) fn clear(&mut self) {
        *self = CachedExecutors::default();
    }

    pub(crate) fn get_primer_count_upper_bound_inclusive(counts: &[(&Pubkey, u64)]) -> u64 {
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

    pub(crate) fn get_primer_counts(counts: &[(&Pubkey, u64)], num_counts: usize) -> Vec<u64> {
        let max_primer_count = Self::get_primer_count_upper_bound_inclusive(counts);
        let mut rng = rand::thread_rng();

        (0..num_counts)
            .map(|_| rng.gen_range(0, max_primer_count.saturating_add(1)))
            .collect::<Vec<_>>()
    }

    pub(crate) fn submit_stats(&self, slot: Slot) {
        self.stats.submit(slot)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
        crate::bank::Bank,
        solana_program_runtime::invoke_context::{Executors, InvokeContext, TransactionExecutor},
        solana_sdk::{
            account::{Account, AccountSharedData},
            bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
            genesis_config::create_genesis_config,
            instruction::InstructionError,
            native_loader,
        },
        std::{cell::RefCell, rc::Rc},
    };
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
            .filter_map(|key| cache.get(key))
            .count();
        assert_eq!(num_retained, 2);

        assert!(cache.get(&key4).is_some());
        assert!(cache.get(&key4).is_some());
        assert!(cache.get(&key4).is_some());
        cache.put(&[(&key3, executor.clone())]);
        assert!(cache.get(&key3).is_some());
        let num_retained = [&key1, &key2, &key4]
            .iter()
            .filter_map(|key| cache.get(key))
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

        let mut cache = CachedExecutors::new_from_parent_bank_executors(&cache, 1);
        assert!(cache.current_epoch == 1);

        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_some());
        cache.put(&[(&key4, executor.clone())]);

        assert!(cache.get(&key4).is_some());
        let num_retained = [&key1, &key2, &key3]
            .iter()
            .filter_map(|key| cache.get(key))
            .count();
        assert_eq!(num_retained, 2);

        cache.put(&[(&key1, executor.clone())]);
        cache.put(&[(&key3, executor.clone())]);
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key3).is_some());
        let num_retained = [&key2, &key4]
            .iter()
            .filter_map(|key| cache.get(key))
            .count();
        assert_eq!(num_retained, 1);

        cache = CachedExecutors::new_from_parent_bank_executors(&cache, 2);
        assert!(cache.current_epoch == 2);

        cache.put(&[(&key3, executor.clone())]);
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

    #[test]
    fn test_cached_executors_stats() {
        #[derive(Debug, Default, PartialEq)]
        struct ComparableStats {
            hits: u64,
            misses: u64,
            evictions: HashMap<Pubkey, u64>,
            insertions: u64,
            replacements: u64,
            one_hit_wonders: u64,
        }
        impl From<&executor_cache::Stats> for ComparableStats {
            fn from(stats: &executor_cache::Stats) -> Self {
                let executor_cache::Stats {
                    hits,
                    misses,
                    evictions,
                    insertions,
                    replacements,
                    one_hit_wonders,
                } = stats;
                ComparableStats {
                    hits: hits.load(Relaxed),
                    misses: misses.load(Relaxed),
                    evictions: evictions.clone(),
                    insertions: insertions.load(Relaxed),
                    replacements: replacements.load(Relaxed),
                    one_hit_wonders: one_hit_wonders.load(Relaxed),
                }
            }
        }

        const CURRENT_EPOCH: Epoch = 0;
        let mut cache = CachedExecutors::new(2, CURRENT_EPOCH);
        let mut expected_stats = ComparableStats::default();

        let program_id1 = Pubkey::new_unique();
        let program_id2 = Pubkey::new_unique();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});

        // make sure we're starting from where we think we are
        assert_eq!(ComparableStats::from(&cache.stats), expected_stats,);

        // insert some executors
        cache.put(&[(&program_id1, executor.clone())]);
        cache.put(&[(&program_id2, executor.clone())]);
        expected_stats.insertions += 2;
        assert_eq!(ComparableStats::from(&cache.stats), expected_stats);

        // replace a one-hit-wonder executor
        cache.put(&[(&program_id1, executor.clone())]);
        expected_stats.replacements += 1;
        expected_stats.one_hit_wonders += 1;
        assert_eq!(ComparableStats::from(&cache.stats), expected_stats);

        // hit some executors
        cache.get(&program_id1);
        cache.get(&program_id1);
        cache.get(&program_id2);
        expected_stats.hits += 3;
        assert_eq!(ComparableStats::from(&cache.stats), expected_stats);

        // miss an executor
        cache.get(&Pubkey::new_unique());
        expected_stats.misses += 1;
        assert_eq!(ComparableStats::from(&cache.stats), expected_stats);

        // evict an executor
        cache.put(&[(&Pubkey::new_unique(), executor.clone())]);
        expected_stats.insertions += 1;
        expected_stats.evictions.insert(program_id2, 1);
        assert_eq!(ComparableStats::from(&cache.stats), expected_stats);

        // make sure stats are cleared in new_from_parent
        assert_eq!(
            ComparableStats::from(
                &CachedExecutors::new_from_parent_bank_executors(&cache, CURRENT_EPOCH).stats
            ),
            ComparableStats::default()
        );
        assert_eq!(
            ComparableStats::from(
                &CachedExecutors::new_from_parent_bank_executors(&cache, CURRENT_EPOCH + 1).stats
            ),
            ComparableStats::default()
        );
    }

    #[derive(Debug)]
    struct TestExecutor {}
    impl Executor for TestExecutor {
        fn execute(
            &self,
            _first_instruction_account: usize,
            _invoke_context: &mut InvokeContext,
        ) -> std::result::Result<(), InstructionError> {
            Ok(())
        }
    }

    #[test]
    fn test_cached_executors_one_hit_wonder_counter() {
        let mut cache = CachedExecutors::new(1, 0);

        let one_hit_wonder = Pubkey::new_unique();
        let popular = Pubkey::new_unique();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});

        // make sure we're starting from where we think we are
        assert_eq!(cache.stats.one_hit_wonders.load(Relaxed), 0);

        // add our one-hit-wonder
        cache.put(&[(&one_hit_wonder, executor.clone())]);
        assert_eq!(cache.executors[&one_hit_wonder].hit_count.load(Relaxed), 1);
        // displace the one-hit-wonder with "popular program"
        cache.put(&[(&popular, executor.clone())]);
        assert_eq!(cache.executors[&popular].hit_count.load(Relaxed), 1);

        // one-hit-wonder counter incremented
        assert_eq!(cache.stats.one_hit_wonders.load(Relaxed), 1);

        // make "popular program" popular
        cache.get(&popular).unwrap();
        assert_eq!(cache.executors[&popular].hit_count.load(Relaxed), 2);

        // evict "popular program"
        cache.put(&[(&one_hit_wonder, executor.clone())]);
        assert_eq!(cache.executors[&one_hit_wonder].hit_count.load(Relaxed), 1);

        // one-hit-wonder counter not incremented
        assert_eq!(cache.stats.one_hit_wonders.load(Relaxed), 1);
    }

    #[test]
    fn test_bank_executor_cache() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1);
        let bank = Bank::new_for_tests(&genesis_config);

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let key3 = solana_sdk::pubkey::new_rand();
        let key4 = solana_sdk::pubkey::new_rand();
        let key5 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});

        fn new_executable_account(owner: Pubkey) -> AccountSharedData {
            AccountSharedData::from(Account {
                owner,
                executable: true,
                ..Account::default()
            })
        }

        let accounts = &[
            (key1, new_executable_account(bpf_loader_upgradeable::id())),
            (key2, new_executable_account(bpf_loader::id())),
            (key3, new_executable_account(bpf_loader_deprecated::id())),
            (key4, new_executable_account(native_loader::id())),
            (key5, AccountSharedData::default()),
        ];

        // don't do any work if not dirty
        let mut executors = Executors::default();
        executors.insert(key1, TransactionExecutor::new_cached(executor.clone()));
        executors.insert(key2, TransactionExecutor::new_cached(executor.clone()));
        executors.insert(key3, TransactionExecutor::new_cached(executor.clone()));
        executors.insert(key4, TransactionExecutor::new_cached(executor.clone()));
        let executors = Rc::new(RefCell::new(executors));
        bank.store_missing_executors(&executors);
        bank.store_updated_executors(&executors);
        let executors = bank.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 0);

        // do work
        let mut executors = Executors::default();
        executors.insert(key1, TransactionExecutor::new_miss(executor.clone()));
        executors.insert(key2, TransactionExecutor::new_miss(executor.clone()));
        executors.insert(key3, TransactionExecutor::new_updated(executor.clone()));
        executors.insert(key4, TransactionExecutor::new_miss(executor.clone()));
        let executors = Rc::new(RefCell::new(executors));

        // store the new_miss
        bank.store_missing_executors(&executors);
        let stored_executors = bank.get_executors(accounts);
        assert_eq!(stored_executors.borrow().len(), 2);
        assert!(stored_executors.borrow().contains_key(&key1));
        assert!(stored_executors.borrow().contains_key(&key2));

        // store the new_updated
        bank.store_updated_executors(&executors);
        let stored_executors = bank.get_executors(accounts);
        assert_eq!(stored_executors.borrow().len(), 3);
        assert!(stored_executors.borrow().contains_key(&key1));
        assert!(stored_executors.borrow().contains_key(&key2));
        assert!(stored_executors.borrow().contains_key(&key3));

        // Check inheritance
        let bank = Bank::new_from_parent(&Arc::new(bank), &solana_sdk::pubkey::new_rand(), 1);
        let executors = bank.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 3);
        assert!(executors.borrow().contains_key(&key1));
        assert!(executors.borrow().contains_key(&key2));
        assert!(executors.borrow().contains_key(&key3));

        // Remove all
        bank.remove_executor(&key1);
        bank.remove_executor(&key2);
        bank.remove_executor(&key3);
        bank.remove_executor(&key4);
        let executors = bank.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 0);
    }

    #[test]
    fn test_bank_executor_cow() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1);
        let root = Arc::new(Bank::new_for_tests(&genesis_config));

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});
        let executable_account = AccountSharedData::from(Account {
            owner: bpf_loader_upgradeable::id(),
            executable: true,
            ..Account::default()
        });

        let accounts = &[
            (key1, executable_account.clone()),
            (key2, executable_account),
        ];

        // add one to root bank
        let mut executors = Executors::default();
        executors.insert(key1, TransactionExecutor::new_miss(executor.clone()));
        let executors = Rc::new(RefCell::new(executors));
        root.store_missing_executors(&executors);
        let executors = root.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 1);

        let fork1 = Bank::new_from_parent(&root, &Pubkey::default(), 1);
        let fork2 = Bank::new_from_parent(&root, &Pubkey::default(), 2);

        let executors = fork1.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 1);
        let executors = fork2.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 1);

        let mut executors = Executors::default();
        executors.insert(key2, TransactionExecutor::new_miss(executor.clone()));
        let executors = Rc::new(RefCell::new(executors));
        fork1.store_missing_executors(&executors);

        let executors = fork1.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 2);
        let executors = fork2.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 1);

        fork1.remove_executor(&key1);

        let executors = fork1.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 1);
        let executors = fork2.get_executors(accounts);
        assert_eq!(executors.borrow().len(), 1);
    }
}
