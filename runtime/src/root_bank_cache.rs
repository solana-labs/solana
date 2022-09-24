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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            accounts_background_service::AbsRequestSender,
            bank_forks::BankForks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        solana_sdk::pubkey::Pubkey,
    };

    #[test]
    fn test_root_bank_cache() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        let mut root_bank_cache = RootBankCache::new(bank_forks.clone());

        let bank = bank_forks.read().unwrap().root_bank();
        assert_eq!(bank, root_bank_cache.root_bank());

        {
            let child_bank = Bank::new_from_parent(&bank, &Pubkey::default(), 1);
            bank_forks.write().unwrap().insert(child_bank);

            // cached slot is still 0 since we have not set root
            assert_eq!(bank.slot(), root_bank_cache.cached_root_bank.slot());
        }
        {
            bank_forks
                .write()
                .unwrap()
                .set_root(1, &AbsRequestSender::default(), None);
            let bank = bank_forks.read().unwrap().root_bank();
            assert!(bank.slot() != root_bank_cache.cached_root_bank.slot());
            assert!(bank != root_bank_cache.cached_root_bank);
            assert_eq!(bank, root_bank_cache.root_bank());
        }
    }
}
