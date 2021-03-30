//! ReadOnlyAccountsCache used to store accounts, such as executable accounts,
//! which can be large, loaded many times, and rarely change.
use dashmap::DashMap;
use std::{
    sync::{Arc, RwLock},
    time::Instant,
};

use solana_sdk::{
    account::{AccountSharedData, ReadableAccount},
    clock::Slot,
    pubkey::Pubkey,
};

#[derive(Debug)]
pub struct ReadOnlyAccountCacheEntry {
    pub account: AccountSharedData,
    pub last_used: Arc<RwLock<Instant>>,
}

#[derive(Debug)]
pub struct ReadOnlyAccountsCache {
    cache: DashMap<(Pubkey, Slot), ReadOnlyAccountCacheEntry>,
    max_data_size: usize,
    data_size: Arc<RwLock<usize>>,
}

impl ReadOnlyAccountsCache {
    pub fn new(max_data_size: usize) -> Self {
        Self {
            max_data_size,
            cache: DashMap::default(),
            data_size: Arc::new(RwLock::new(0)),
        }
    }

    pub fn load(&self, pubkey: &Pubkey, slot: Slot) -> Option<AccountSharedData> {
        self.cache.get(&(*pubkey, slot)).map(|account_ref| {
            let value = account_ref.value();
            // remember last use
            let now = Instant::now();
            *value.last_used.write().unwrap() = now;
            value.account.clone()
        })
    }

    pub fn store(&self, pubkey: &Pubkey, slot: Slot, account: &AccountSharedData) {
        let len = account.data().len();
        self.cache.insert(
            (*pubkey, slot),
            ReadOnlyAccountCacheEntry {
                account: account.clone(),
                last_used: Arc::new(RwLock::new(Instant::now())),
            },
        );

        // maybe purge after we insert. Insert may have replaced.
        let new_size = self.maybe_purge_lru_items(len);
        *self.data_size.write().unwrap() = new_size;
    }

    pub fn remove(&self, pubkey: &Pubkey, slot: Slot) {
        // does not keep track of data size reduction here.
        // data size will be recomputed the next time we store and we think we may now be too large.
        self.cache.remove(&(*pubkey, slot));
    }

    fn maybe_purge_lru_items(&self, new_item_len: usize) -> usize {
        let mut new_size = *self.data_size.read().unwrap() + new_item_len;
        if new_size <= self.max_data_size {
            return new_size;
        }

        // purge in lru order
        let mut lru = Vec::with_capacity(self.cache.len());
        new_size = 0;
        for item in self.cache.iter() {
            let value = item.value();
            let item_len = value.account.data().len();
            new_size += item_len;
            lru.push((*value.last_used.read().unwrap(), item_len, *item.key()));
        }
        if new_size > self.max_data_size {
            lru.sort();
            for oldest in lru.into_iter() {
                self.cache.remove(&oldest.2);
                new_size = new_size.saturating_sub(oldest.1);
                if new_size <= self.max_data_size {
                    break;
                }
            }
        }
        new_size
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub fn data_size(&self) -> usize {
        *self.data_size.read().unwrap()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use solana_sdk::account::{accounts_equal, Account};
    #[test]
    fn test_read_only_accounts_cache() {
        solana_logger::setup();
        let max = 100;
        let cache = ReadOnlyAccountsCache::new(max);
        let slot = 0;
        assert!(cache.load(&Pubkey::default(), slot).is_none());
        assert_eq!(0, cache.cache_len());
        assert_eq!(0, cache.data_size());
        cache.remove(&Pubkey::default(), slot); // assert no panic
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let account1 = AccountSharedData::from(Account {
            data: vec![0; max],
            ..Account::default()
        });
        let mut account2 = account1.clone();
        account2.lamports += 1; // so they compare differently
        let mut account3 = account1.clone();
        account3.lamports += 4; // so they compare differently
        cache.store(&key1, slot, &account1);
        assert_eq!(100, cache.data_size());
        assert!(accounts_equal(&cache.load(&key1, slot).unwrap(), &account1));
        assert_eq!(1, cache.cache_len());
        cache.store(&key2, slot, &account2);
        assert_eq!(100, cache.data_size());
        assert!(accounts_equal(&cache.load(&key2, slot).unwrap(), &account2));
        assert_eq!(1, cache.cache_len());
        cache.store(&key2, slot, &account1); // overwrite key2 with account1
        assert_eq!(100, cache.data_size());
        assert!(accounts_equal(&cache.load(&key2, slot).unwrap(), &account1));
        assert_eq!(1, cache.cache_len());
        cache.remove(&key2, slot);
        assert_eq!(100, cache.data_size());
        assert_eq!(0, cache.cache_len());

        // can store 2 items, 3rd item kicks oldest item out
        let max = 200;
        let cache = ReadOnlyAccountsCache::new(max);
        cache.store(&key1, slot, &account1);
        assert_eq!(100, cache.data_size());
        assert!(accounts_equal(&cache.load(&key1, slot).unwrap(), &account1));
        assert_eq!(1, cache.cache_len());
        cache.store(&key2, slot, &account2);
        assert_eq!(200, cache.data_size());
        assert!(accounts_equal(&cache.load(&key1, slot).unwrap(), &account1));
        assert!(accounts_equal(&cache.load(&key2, slot).unwrap(), &account2));
        assert_eq!(2, cache.cache_len());
        cache.store(&key2, slot, &account1); // overwrite key2 with account1
        assert_eq!(200, cache.data_size());
        assert!(accounts_equal(&cache.load(&key1, slot).unwrap(), &account1));
        assert!(accounts_equal(&cache.load(&key2, slot).unwrap(), &account1));
        assert_eq!(2, cache.cache_len());
        cache.store(&key3, slot, &account3);
        assert_eq!(200, cache.data_size());
        assert!(cache.load(&key1, slot).is_none()); // was lru purged
        assert!(accounts_equal(&cache.load(&key2, slot).unwrap(), &account1));
        assert!(accounts_equal(&cache.load(&key3, slot).unwrap(), &account3));
        assert_eq!(2, cache.cache_len());
    }
}
