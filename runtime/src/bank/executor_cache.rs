use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering::Relaxed},
        Arc, LockResult, RwLock, RwLockReadGuard, RwLockWriteGuard,
    },
};

use solana_sdk::{
    account::AccountSharedData, message::Message, process_instruction::Executor, pubkey::Pubkey,
};

use super::Bank;
use crate::message_processor::Executors;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;

/// Copy-on-write holder of CachedExecutors
#[derive(AbiExample, Debug, Default)]
pub(super) struct CowCachedExecutors {
    shared: bool,
    executors: Arc<RwLock<CachedExecutors>>,
}
impl Clone for CowCachedExecutors {
    fn clone(&self) -> Self {
        Self {
            shared: true,
            executors: self.executors.clone(),
        }
    }
}
impl CowCachedExecutors {
    pub(super) fn new(executors: Arc<RwLock<CachedExecutors>>) -> Self {
        Self {
            shared: true,
            executors,
        }
    }
    pub(super) fn read(&self) -> LockResult<RwLockReadGuard<CachedExecutors>> {
        self.executors.read()
    }
    pub(super) fn write(&mut self) -> LockResult<RwLockWriteGuard<CachedExecutors>> {
        if self.shared {
            self.shared = false;
            let local_cache = (*self.executors.read().unwrap()).clone();
            self.executors = Arc::new(RwLock::new(local_cache));
        }
        self.executors.write()
    }
}

pub(super) const MAX_CACHED_EXECUTORS: usize = 100; // 10 MB assuming programs are around 100k

/// LFU Cache of executors
#[derive(Debug)]
pub(super) struct CachedExecutors {
    max: usize,
    executors: HashMap<Pubkey, (AtomicU64, Arc<dyn Executor>)>,
}
impl Default for CachedExecutors {
    fn default() -> Self {
        Self {
            max: MAX_CACHED_EXECUTORS,
            executors: HashMap::new(),
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
        let mut executors = HashMap::new();
        for (key, (count, executor)) in self.executors.iter() {
            executors.insert(
                *key,
                (AtomicU64::new(count.load(Relaxed)), executor.clone()),
            );
        }
        Self {
            max: self.max,
            executors,
        }
    }
}
impl CachedExecutors {
    pub(super) fn new(max: usize) -> Self {
        Self {
            max,
            executors: HashMap::new(),
        }
    }
    pub(super) fn get(&self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        self.executors.get(pubkey).map(|(count, executor)| {
            count.fetch_add(1, Relaxed);
            executor.clone()
        })
    }
    pub(super) fn put(&mut self, pubkey: &Pubkey, executor: Arc<dyn Executor>) {
        if !self.executors.contains_key(pubkey) && self.executors.len() >= self.max {
            let mut least = u64::MAX;
            let default_key = Pubkey::default();
            let mut least_key = &default_key;
            for (key, (count, _)) in self.executors.iter() {
                let count = count.load(Relaxed);
                if count < least {
                    least = count;
                    least_key = key;
                }
            }
            let least_key = *least_key;
            let _ = self.executors.remove(&least_key);
        }
        let _ = self
            .executors
            .insert(*pubkey, (AtomicU64::new(0), executor));
    }
    pub(super) fn remove(&mut self, pubkey: &Pubkey) {
        let _ = self.executors.remove(pubkey);
    }
}

impl Bank {
    /// Add executors back to the bank's cache if modified
    pub(super) fn update_executors(&self, executors: Rc<RefCell<Executors>>) {
        let executors = executors.borrow();
        if executors.is_dirty {
            let mut cow_cache = self.cached_executors.write().unwrap();
            let mut cache = cow_cache.write().unwrap();
            for (key, executor) in executors.executors.iter() {
                cache.put(key, (*executor).clone());
            }
        }
    }

    /// Remove an executor from the bank's cache
    pub fn remove_executor(&self, pubkey: &Pubkey) {
        let mut cow_cache = self.cached_executors.write().unwrap();
        let mut cache = cow_cache.write().unwrap();
        cache.remove(pubkey);
    }

    /// Get any cached executors needed by the transaction
    pub(super) fn get_executors(
        &self,
        message: &Message,
        loaders: &[Vec<(Pubkey, AccountSharedData)>],
    ) -> Rc<RefCell<Executors>> {
        let mut num_executors = message.account_keys.len();
        for instruction_loaders in loaders.iter() {
            num_executors += instruction_loaders.len();
        }
        let mut executors = HashMap::with_capacity(num_executors);
        let cow_cache = self.cached_executors.read().unwrap();
        let cache = cow_cache.read().unwrap();

        for key in message.account_keys.iter() {
            if let Some(executor) = cache.get(key) {
                executors.insert(*key, executor);
            }
        }
        for instruction_loaders in loaders.iter() {
            for (key, _) in instruction_loaders.iter() {
                if let Some(executor) = cache.get(key) {
                    executors.insert(*key, executor);
                }
            }
        }

        Rc::new(RefCell::new(Executors {
            executors,
            is_dirty: false,
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use solana_sdk::{
        genesis_config::create_genesis_config,
        hash::Hash,
        instruction::InstructionError,
        message::{Message, MessageHeader},
        process_instruction::InvokeContext,
    };
    #[derive(Debug)]
    pub(super) struct TestExecutor {}
    impl Executor for TestExecutor {
        fn execute(
            &self,
            _loader_id: &Pubkey,
            _program_id: &Pubkey,
            _instruction_data: &[u8],
            _invoke_context: &mut dyn InvokeContext,
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
        let mut cache = CachedExecutors::new(3);

        cache.put(&key1, executor.clone());
        cache.put(&key2, executor.clone());
        cache.put(&key3, executor.clone());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_some());

        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());
        cache.put(&key4, executor.clone());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());
        assert!(cache.get(&key3).is_none());
        assert!(cache.get(&key4).is_some());

        assert!(cache.get(&key4).is_some());
        assert!(cache.get(&key4).is_some());
        assert!(cache.get(&key4).is_some());
        cache.put(&key3, executor.clone());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_none());
        assert!(cache.get(&key3).is_some());
        assert!(cache.get(&key4).is_some());
    }

    #[test]
    fn test_bank_executor_cache() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1);
        let bank = Bank::new(&genesis_config);

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let key3 = solana_sdk::pubkey::new_rand();
        let key4 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});

        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key1, key2],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };

        let loaders = &[
            vec![
                (key3, AccountSharedData::default()),
                (key4, AccountSharedData::default()),
            ],
            vec![(key1, AccountSharedData::default())],
        ];

        // don't do any work if not dirty
        let mut executors = Executors::default();
        executors.insert(key1, executor.clone());
        executors.insert(key2, executor.clone());
        executors.insert(key3, executor.clone());
        executors.insert(key4, executor.clone());
        let executors = Rc::new(RefCell::new(executors));
        executors.borrow_mut().is_dirty = false;
        bank.update_executors(executors);
        let executors = bank.get_executors(&message, loaders);
        assert_eq!(executors.borrow().executors.len(), 0);

        // do work
        let mut executors = Executors::default();
        executors.insert(key1, executor.clone());
        executors.insert(key2, executor.clone());
        executors.insert(key3, executor.clone());
        executors.insert(key4, executor.clone());
        let executors = Rc::new(RefCell::new(executors));
        bank.update_executors(executors);
        let executors = bank.get_executors(&message, loaders);
        assert_eq!(executors.borrow().executors.len(), 4);
        assert!(executors.borrow().executors.contains_key(&key1));
        assert!(executors.borrow().executors.contains_key(&key2));
        assert!(executors.borrow().executors.contains_key(&key3));
        assert!(executors.borrow().executors.contains_key(&key4));

        // Check inheritance
        let bank = Bank::new_from_parent(&Arc::new(bank), &solana_sdk::pubkey::new_rand(), 1);
        let executors = bank.get_executors(&message, loaders);
        assert_eq!(executors.borrow().executors.len(), 4);
        assert!(executors.borrow().executors.contains_key(&key1));
        assert!(executors.borrow().executors.contains_key(&key2));
        assert!(executors.borrow().executors.contains_key(&key3));
        assert!(executors.borrow().executors.contains_key(&key4));

        bank.remove_executor(&key1);
        bank.remove_executor(&key2);
        bank.remove_executor(&key3);
        bank.remove_executor(&key4);
        let executors = bank.get_executors(&message, loaders);
        assert_eq!(executors.borrow().executors.len(), 0);
        assert!(!executors.borrow().executors.contains_key(&key1));
        assert!(!executors.borrow().executors.contains_key(&key2));
        assert!(!executors.borrow().executors.contains_key(&key3));
        assert!(!executors.borrow().executors.contains_key(&key4));
    }

    #[test]
    fn test_bank_executor_cow() {
        solana_logger::setup();

        let (genesis_config, _) = create_genesis_config(1);
        let root = Arc::new(Bank::new(&genesis_config));

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let executor: Arc<dyn Executor> = Arc::new(TestExecutor {});

        let loaders = &[vec![
            (key1, AccountSharedData::default()),
            (key2, AccountSharedData::default()),
        ]];

        // add one to root bank
        let mut executors = Executors::default();
        executors.insert(key1, executor.clone());
        let executors = Rc::new(RefCell::new(executors));
        root.update_executors(executors);
        let executors = root.get_executors(&Message::default(), loaders);
        assert_eq!(executors.borrow().executors.len(), 1);

        let fork1 = Bank::new_from_parent(&root, &Pubkey::default(), 1);
        let fork2 = Bank::new_from_parent(&root, &Pubkey::default(), 1);

        let executors = fork1.get_executors(&Message::default(), loaders);
        assert_eq!(executors.borrow().executors.len(), 1);
        let executors = fork2.get_executors(&Message::default(), loaders);
        assert_eq!(executors.borrow().executors.len(), 1);

        let mut executors = Executors::default();
        executors.insert(key2, executor.clone());
        let executors = Rc::new(RefCell::new(executors));
        fork1.update_executors(executors);

        let executors = fork1.get_executors(&Message::default(), loaders);
        assert_eq!(executors.borrow().executors.len(), 2);
        let executors = fork2.get_executors(&Message::default(), loaders);
        assert_eq!(executors.borrow().executors.len(), 1);

        fork1.remove_executor(&key1);

        let executors = fork1.get_executors(&Message::default(), loaders);
        assert_eq!(executors.borrow().executors.len(), 1);
        let executors = fork2.get_executors(&Message::default(), loaders);
        assert_eq!(executors.borrow().executors.len(), 1);
    }
}
