//! A wrapper around a root `Bank` that only loads from bank forks if the root has been updated.
//! This can be useful to avoid read-locking the bank forks until the root has been updated.
//!

use {
    crate::{
        bank::Bank,
        bank_forks::{BankForks, ReadOnlyAtomicSlot},
    },
    std::sync::{Arc, RwLock, Weak},
};

/// Cached root bank that only loads from bank forks if the root has been updated.
pub struct RootBankCache {
    bank_forks: Arc<RwLock<BankForks>>,
    cached_root_bank: Weak<Bank>,
    root_slot: ReadOnlyAtomicSlot,
}

impl RootBankCache {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        let (cached_root_bank, root_slot) = {
            let lock = bank_forks.read().unwrap();
            (Arc::downgrade(&lock.root_bank()), lock.get_atomic_root())
        };
        Self {
            bank_forks,
            cached_root_bank,
            root_slot,
        }
    }

    pub fn root_bank(&mut self) -> Arc<Bank> {
        match self.cached_root_bank.upgrade() {
            Some(cached_root_bank) if cached_root_bank.slot() == self.root_slot.get() => {
                cached_root_bank
            }
            _ => {
                let root_bank = self.bank_forks.read().unwrap().root_bank();
                self.cached_root_bank = Arc::downgrade(&root_bank);
                root_bank
            }
        }
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
        let bank_forks = BankForks::new_rw_arc(bank);

        let mut root_bank_cache = RootBankCache::new(bank_forks.clone());

        let bank = bank_forks.read().unwrap().root_bank();
        assert_eq!(bank, root_bank_cache.root_bank());

        {
            let child_bank = Bank::new_from_parent(bank.clone(), &Pubkey::default(), 1);
            bank_forks.write().unwrap().insert(child_bank);

            // cached slot is still 0 since we have not set root
            let cached_root_bank = root_bank_cache.cached_root_bank.upgrade().unwrap();
            assert_eq!(bank.slot(), cached_root_bank.slot());
        }
        {
            bank_forks
                .write()
                .unwrap()
                .set_root(1, &AbsRequestSender::default(), None);
            let bank = bank_forks.read().unwrap().root_bank();

            // cached slot and bank are not updated until we call `root_bank()`
            let cached_root_bank = root_bank_cache.cached_root_bank.upgrade().unwrap();
            assert!(bank.slot() != cached_root_bank.slot());
            assert!(bank != cached_root_bank);
            assert_eq!(bank, root_bank_cache.root_bank());

            // cached slot and bank are updated
            let cached_root_bank = root_bank_cache.cached_root_bank.upgrade().unwrap();
            assert_eq!(bank.slot(), cached_root_bank.slot());
            assert_eq!(bank, cached_root_bank);
            assert_eq!(bank, root_bank_cache.root_bank());
        }
    }
}
