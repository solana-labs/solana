//! A wrapper around bank forks that maintains a lightweight cache of bank hashes.
//!
//! Notified by bank forks when a slot is dumped due to duplicate block handling, allowing for the
//! cache to be invalidated. This ensures that the cache is always in sync with bank forks as
//! long as the local lock is held during querying.
//!
//! This can be useful to avoid read-locking the bank forks when querying bank hashes, as we only
//! contend for the local lock during slot dumping due to duplicate blocks which should be extremely rare.

use {
    crate::{bank::Bank, bank_forks::BankForks, root_bank_cache::RootBankCache},
    solana_sdk::{clock::Slot, hash::Hash},
    std::{
        collections::BTreeMap,
        sync::{Arc, Mutex, MutexGuard, RwLock},
    },
};

// Used to notify bank hash cache that slots have been dumped by replay
pub type DumpedSlotSubscription = Arc<Mutex<bool>>;

pub struct BankHashCache {
    hashes: BTreeMap<Slot, Hash>,
    bank_forks: Arc<RwLock<BankForks>>,
    root_bank_cache: RootBankCache,
    last_root: Slot,
    dumped_slot_subscription: DumpedSlotSubscription,
}

impl BankHashCache {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        let root_bank_cache = RootBankCache::new(bank_forks.clone());
        let dumped_slot_subscription = DumpedSlotSubscription::default();
        bank_forks
            .write()
            .unwrap()
            .register_dumped_slot_subscriber(dumped_slot_subscription.clone());
        Self {
            hashes: BTreeMap::default(),
            bank_forks,
            root_bank_cache,
            last_root: 0,
            dumped_slot_subscription,
        }
    }

    pub fn dumped_slot_subscription(&self) -> DumpedSlotSubscription {
        self.dumped_slot_subscription.clone()
    }

    /// Should only be used after `slots_dumped` is acquired from `dumped_slot_subscription` to
    /// guarantee synchronicity with `self.bank_forks`. Multiple calls to `hash` will only be
    /// consistent with each other if `slots_dumped` was not released in between, as otherwise a dump
    /// could have occured inbetween.
    pub fn hash(&mut self, slot: Slot, slots_dumped: &mut MutexGuard<bool>) -> Option<Hash> {
        if **slots_dumped {
            // We could be smarter and keep a fork cache to only clear affected slots from the cache,
            // but dumping slots should be extremely rare so it is simpler to invalidate the entire cache.
            self.hashes.clear();
            **slots_dumped = false;
        }

        if let Some(hash) = self.hashes.get(&slot) {
            return Some(*hash);
        }

        let Some(hash) = self.bank_forks.read().unwrap().bank_hash(slot) else {
            // Bank not yet received, bail
            return None;
        };

        if hash == Hash::default() {
            // If we have not frozen the bank then bail
            return None;
        }

        // Cache the slot for future lookup
        let prev_hash = self.hashes.insert(slot, hash);
        debug_assert!(
            prev_hash.is_none(),
            "Programmer error, this indicates we have dumped and replayed \
             a block however the cache was not invalidated"
        );
        Some(hash)
    }

    pub fn root(&mut self) -> Slot {
        self.get_root_bank_and_prune_cache().slot()
    }

    /// Returns the root bank and also prunes cache of any slots < root
    pub fn get_root_bank_and_prune_cache(&mut self) -> Arc<Bank> {
        let root_bank = self.root_bank_cache.root_bank();
        if root_bank.slot() != self.last_root {
            self.last_root = root_bank.slot();
            self.hashes = self.hashes.split_off(&self.last_root);
        }
        root_bank
    }
}
