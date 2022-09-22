//! A wrapper around a root `Bank` that only loads from bank forks if the root has been updated.
//! This can be useful to avoid read-locking the bank forks until the root has been updated.
//!

use {
    crate::{
        bank::Bank,
        bank_forks::{BankForks, ReadOnlyAtomicSlot},
    },
    std::sync::{Arc, RwLock},
};

/// Cached root bank that only loads from bank forks if the root has been updated.
pub struct RootBankCache {
    bank_forks: Arc<RwLock<BankForks>>,
    cached_root_bank: Arc<Bank>,
    root_slot: ReadOnlyAtomicSlot,
}

impl RootBankCache {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        let (cached_root_bank, root_slot) = {
            let lock = bank_forks.read().unwrap();
            (lock.root_bank(), lock.get_atomic_root())
        };
        Self {
            bank_forks,
            cached_root_bank,
            root_slot,
        }
    }

    pub fn root_bank(&mut self) -> Arc<Bank> {
        let current_root_slot = self.root_slot.get();
        if self.cached_root_bank.slot() != current_root_slot {
            let lock = self.bank_forks.read().unwrap();
            self.cached_root_bank = lock.root_bank();
        }
        self.cached_root_bank.clone()
    }
}
