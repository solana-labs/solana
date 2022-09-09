#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
use {
    indexmap::{map::Entry, IndexMap},
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

pub(crate) mod executor_cache {
    use {super::*, log};

    #[derive(Debug, Default)]
    pub struct Stats {
        pub hits: AtomicU64,
        pub num_gets: AtomicU64,
        pub evictions: HashMap<Pubkey, u64>,
        pub insertions: AtomicU64,
        pub replacements: AtomicU64,
        pub one_hit_wonders: AtomicU64,
    }

    impl Stats {
        pub fn submit(&self, slot: Slot) {
            let hits = self.hits.load(Relaxed);
            let misses = self.num_gets.load(Relaxed) - hits;
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

#[derive(Debug, Default)]
pub(crate) struct CachedExecutorsEntry {
    prev_epoch_count: u64,
    epoch_count: AtomicU64,
    hit_count: AtomicU64,
}

impl Clone for CachedExecutorsEntry {
    fn clone(&self) -> Self {
        Self {
            prev_epoch_count: self.prev_epoch_count,
            epoch_count: AtomicU64::new(self.epoch_count.load(Relaxed)),
            hit_count: AtomicU64::new(self.hit_count.load(Relaxed)),
        }
    }
}

/// LFU Cache of executors with single-epoch memory of usage counts
#[derive(Debug)]
pub(crate) struct CachedExecutors {
    capacity: usize,
    current_epoch: Epoch,
    executors: IndexMap<Pubkey, Arc<dyn Executor>>,
    entries: IndexMap<Pubkey, CachedExecutorsEntry>,
    stats: executor_cache::Stats,
}

impl Default for CachedExecutors {
    fn default() -> Self {
        Self {
            capacity: MAX_CACHED_EXECUTORS,
            current_epoch: Epoch::default(),
            executors: IndexMap::new(),
            entries: IndexMap::new(),
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
            entries: IndexMap::new(),
            stats: executor_cache::Stats::default(),
        }
    }

    pub(crate) fn new_from_parent_bank_executors(
        parent_bank_executors: &CachedExecutors,
        current_epoch: Epoch,
    ) -> Self {
        let entries = if parent_bank_executors.current_epoch == current_epoch {
            parent_bank_executors.entries.clone()
        } else {
            parent_bank_executors
                .entries
                .iter()
                .map(|(&key, entry)| {
                    let entry = CachedExecutorsEntry {
                        prev_epoch_count: entry.epoch_count.load(Relaxed),
                        epoch_count: AtomicU64::default(),
                        hit_count: AtomicU64::new(entry.hit_count.load(Relaxed)),
                    };
                    (key, entry)
                })
                .collect()
        };

        Self {
            capacity: parent_bank_executors.capacity,
            current_epoch,
            executors: parent_bank_executors.executors.clone(),
            entries,
            stats: executor_cache::Stats::default(),
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
            self.entries.entry(pubkey).or_default().hit_count.fetch_add(1, Relaxed);
            
            match self.executors.entry(pubkey) {
                Entry::Vacant(entry) => {
                    self.stats.insertions.fetch_add(1, Relaxed);
                    entry.insert(executor);
                }
                Entry::Occupied(mut entry) => {
                    self.stats.replacements.fetch_add(1, Relaxed);
                    *entry.get_mut() = executor;
                }
            }
        }

        let mut rng = rand::thread_rng();
        // Evict the key with the lowest hits in a random sample of entries.
        while self.executors.len() > self.capacity {
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

        while self.entries.len() > self.capacity.saturating_mul(20) {
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

    pub(crate) fn remove(&mut self, pubkey: &Pubkey) -> Option<CachedExecutorsEntry> {
        let maybe_entry = self.entries.remove(pubkey);
        if let Some(entry) = maybe_entry.as_ref() {
            if entry.hit_count.load(Relaxed) == 1 {
                self.stats.one_hit_wonders.fetch_add(1, Relaxed);
            }
        }

        self.executors.remove(pubkey);
        maybe_entry
    }

    pub(crate) fn clear(&mut self) {
        *self = CachedExecutors::default();
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
            .filter_map(|key| cache.get(key))
            .count();
        assert_eq!(num_retained, 3);

        cache.put(vec![(key3, executor.clone())]);
        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .filter_map(|key| cache.get(key))
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

        let mut cache = CachedExecutors::new_from_parent_bank_executors(&cache, 1);
        assert!(cache.current_epoch == 1);

        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_some());
        cache.put(vec![(key4, executor.clone())]);

        assert!(cache.get(&key4).is_some());
        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .filter_map(|key| cache.get(key))
            .count();
        assert_eq!(num_retained, 3);

        cache.put(vec![(key1, executor.clone())]);
        cache.put(vec![(key3, executor.clone())]);
        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .filter_map(|key| cache.get(key))
            .count();
        assert_eq!(num_retained, 3);

        cache = CachedExecutors::new_from_parent_bank_executors(&cache, 2);
        assert!(cache.current_epoch == 2);

        cache.put(vec![(key3, executor.clone())]);

        let num_retained = [&key1, &key2, &key3, &key4]
            .iter()
            .filter_map(|key| cache.get(key))
            .count();
        assert_eq!(num_retained, 3);
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
                    num_gets,
                    evictions,
                    insertions,
                    replacements,
                    one_hit_wonders,
                } = stats;
                ComparableStats {
                    hits: hits.load(Relaxed),
                    misses: num_gets.load(Relaxed) - hits.load(Relaxed),
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
        cache.put(vec![(program_id1, executor.clone())]);
        cache.put(vec![(program_id2, executor.clone())]);
        expected_stats.insertions += 2;
        assert_eq!(ComparableStats::from(&cache.stats), expected_stats);

        // replace a one-hit-wonder executor
        cache.put(vec![(program_id1, executor.clone())]);
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
        cache.put(vec![(Pubkey::new_unique(), executor.clone())]);
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
            _first_instruction_account: u16,
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
        cache.put(vec![(one_hit_wonder, executor.clone())]);
        assert_eq!(cache.entries[&one_hit_wonder].hit_count.load(Relaxed), 1);
        // displace the one-hit-wonder with "popular program"
        cache.put(vec![(popular, executor.clone())]);
        assert_eq!(cache.entries[&popular].hit_count.load(Relaxed), 1);

        // one-hit-wonder counter incremented
        assert_eq!(cache.stats.one_hit_wonders.load(Relaxed), 1);

        // make "popular program" popular
        cache.get(&popular).unwrap();
        assert_eq!(cache.entries[&popular].hit_count.load(Relaxed), 2);

        // evict "popular program"
        cache.put(vec![(one_hit_wonder, executor.clone())]);
        assert_eq!(cache.entries[&one_hit_wonder].hit_count.load(Relaxed), 1);

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
