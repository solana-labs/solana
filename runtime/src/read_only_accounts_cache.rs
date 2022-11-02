//! ReadOnlyAccountsCache used to store accounts, such as executable accounts,
//! which can be large, loaded many times, and rarely change.
use {
    dashmap::{mapref::entry::Entry, DashMap},
    index_list::{Index, IndexList},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        pubkey::Pubkey,
    },
    std::sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Mutex,
    },
};

const CACHE_ENTRY_SIZE: usize =
    std::mem::size_of::<ReadOnlyAccountCacheEntry>() + 2 * std::mem::size_of::<ReadOnlyCacheKey>();

type ReadOnlyCacheKey = (Pubkey, Slot);

#[derive(Debug)]
struct ReadOnlyAccountCacheEntry {
    account: AccountSharedData,
    index: Index, // Index of the entry in the eviction queue.
}

#[derive(Debug)]
pub(crate) struct ReadOnlyAccountsCache {
    cache: DashMap<ReadOnlyCacheKey, ReadOnlyAccountCacheEntry>,
    // When an item is first entered into the cache, it is added to the end of
    // the queue. Also each time an entry is looked up from the cache it is
    // moved to the end of the queue. As a result, items in the queue are
    // always sorted in the order that they have last been accessed. When doing
    // LRU eviction, cache entries are evicted from the front of the queue.
    queue: Mutex<IndexList<ReadOnlyCacheKey>>,
    max_data_size: usize,
    data_size: AtomicUsize,
    hits: AtomicU64,
    misses: AtomicU64,
    evicts: AtomicU64,
}

impl ReadOnlyAccountsCache {
    pub(crate) fn new(max_data_size: usize) -> Self {
        Self {
            max_data_size,
            cache: DashMap::default(),
            queue: Mutex::<IndexList<ReadOnlyCacheKey>>::default(),
            data_size: AtomicUsize::default(),
            hits: AtomicU64::default(),
            misses: AtomicU64::default(),
            evicts: AtomicU64::default(),
        }
    }

    /// reset the read only accounts cache
    /// useful for benches/tests
    pub fn reset_for_tests(&self) {
        self.cache.clear();
        self.queue.lock().unwrap().clear();
        self.data_size.store(0, Ordering::Relaxed);
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evicts.store(0, Ordering::Relaxed);
    }

    /// true if pubkey is in cache at slot
    pub fn in_cache(&self, pubkey: &Pubkey, slot: Slot) -> bool {
        self.cache.contains_key(&(*pubkey, slot))
    }

    pub(crate) fn load(&self, pubkey: Pubkey, slot: Slot) -> Option<AccountSharedData> {
        let key = (pubkey, slot);
        let mut entry = match self.cache.get_mut(&key) {
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            Some(entry) => entry,
        };
        self.hits.fetch_add(1, Ordering::Relaxed);
        // Move the entry to the end of the queue.
        // self.queue is modified while holding a reference to the cache entry;
        // so that another thread cannot write to the same key.
        {
            let mut queue = self.queue.lock().unwrap();
            queue.remove(entry.index);
            entry.index = queue.insert_last(key);
        }
        Some(entry.account.clone())
    }

    fn account_size(&self, account: &AccountSharedData) -> usize {
        CACHE_ENTRY_SIZE + account.data().len()
    }

    pub(crate) fn store(&self, pubkey: Pubkey, slot: Slot, account: AccountSharedData) {
        let key = (pubkey, slot);
        let account_size = self.account_size(&account);
        self.data_size.fetch_add(account_size, Ordering::Relaxed);
        // self.queue is modified while holding a reference to the cache entry;
        // so that another thread cannot write to the same key.
        match self.cache.entry(key) {
            Entry::Vacant(entry) => {
                // Insert the entry at the end of the queue.
                let mut queue = self.queue.lock().unwrap();
                let index = queue.insert_last(key);
                entry.insert(ReadOnlyAccountCacheEntry { account, index });
            }
            Entry::Occupied(mut entry) => {
                let entry = entry.get_mut();
                let account_size = self.account_size(&entry.account);
                self.data_size.fetch_sub(account_size, Ordering::Relaxed);
                entry.account = account;
                // Move the entry to the end of the queue.
                let mut queue = self.queue.lock().unwrap();
                queue.remove(entry.index);
                entry.index = queue.insert_last(key);
            }
        };
        // Evict entries from the front of the queue.
        let mut num_evicts = 0;
        while self.data_size.load(Ordering::Relaxed) > self.max_data_size {
            let (pubkey, slot) = match self.queue.lock().unwrap().get_first() {
                None => break,
                Some(key) => *key,
            };
            num_evicts += 1;
            self.remove(pubkey, slot);
        }
        self.evicts.fetch_add(num_evicts, Ordering::Relaxed);
    }

    pub(crate) fn remove(&self, pubkey: Pubkey, slot: Slot) -> Option<AccountSharedData> {
        let (_, entry) = self.cache.remove(&(pubkey, slot))?;
        // self.queue should be modified only after removing the entry from the
        // cache, so that this is still safe if another thread writes to the
        // same key.
        self.queue.lock().unwrap().remove(entry.index);
        let account_size = self.account_size(&entry.account);
        self.data_size.fetch_sub(account_size, Ordering::Relaxed);
        Some(entry.account)
    }

    pub(crate) fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub(crate) fn data_size(&self) -> usize {
        self.data_size.load(Ordering::Relaxed)
    }

    pub(crate) fn get_and_reset_stats(&self) -> (u64, u64, u64) {
        let hits = self.hits.swap(0, Ordering::Relaxed);
        let misses = self.misses.swap(0, Ordering::Relaxed);
        let evicts = self.evicts.swap(0, Ordering::Relaxed);

        (hits, misses, evicts)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        rand::{
            seq::{IteratorRandom, SliceRandom},
            Rng, SeedableRng,
        },
        rand_chacha::ChaChaRng,
        solana_sdk::account::{accounts_equal, Account, WritableAccount},
        std::{collections::HashMap, iter::repeat_with, sync::Arc},
    };
    #[test]
    fn test_accountsdb_sizeof() {
        // size_of(arc(x)) does not return the size of x
        assert!(std::mem::size_of::<Arc<u64>>() == std::mem::size_of::<Arc<u8>>());
        assert!(std::mem::size_of::<Arc<u64>>() == std::mem::size_of::<Arc<[u8; 32]>>());
    }

    #[test]
    fn test_read_only_accounts_cache() {
        solana_logger::setup();
        let per_account_size = CACHE_ENTRY_SIZE;
        let data_size = 100;
        let max = data_size + per_account_size;
        let cache = ReadOnlyAccountsCache::new(max);
        let slot = 0;
        assert!(cache.load(Pubkey::default(), slot).is_none());
        assert_eq!(0, cache.cache_len());
        assert_eq!(0, cache.data_size());
        cache.remove(Pubkey::default(), slot); // assert no panic
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let account1 = AccountSharedData::from(Account {
            data: vec![0; data_size],
            ..Account::default()
        });
        let mut account2 = account1.clone();
        account2.checked_add_lamports(1).unwrap(); // so they compare differently
        let mut account3 = account1.clone();
        account3.checked_add_lamports(4).unwrap(); // so they compare differently
        cache.store(key1, slot, account1.clone());
        assert_eq!(100 + per_account_size, cache.data_size());
        assert!(accounts_equal(&cache.load(key1, slot).unwrap(), &account1));
        assert_eq!(1, cache.cache_len());
        cache.store(key2, slot, account2.clone());
        assert_eq!(100 + per_account_size, cache.data_size());
        assert!(accounts_equal(&cache.load(key2, slot).unwrap(), &account2));
        assert_eq!(1, cache.cache_len());
        cache.store(key2, slot, account1.clone()); // overwrite key2 with account1
        assert_eq!(100 + per_account_size, cache.data_size());
        assert!(accounts_equal(&cache.load(key2, slot).unwrap(), &account1));
        assert_eq!(1, cache.cache_len());
        cache.remove(key2, slot);
        assert_eq!(0, cache.data_size());
        assert_eq!(0, cache.cache_len());

        // can store 2 items, 3rd item kicks oldest item out
        let max = (data_size + per_account_size) * 2;
        let cache = ReadOnlyAccountsCache::new(max);
        cache.store(key1, slot, account1.clone());
        assert_eq!(100 + per_account_size, cache.data_size());
        assert!(accounts_equal(&cache.load(key1, slot).unwrap(), &account1));
        assert_eq!(1, cache.cache_len());
        cache.store(key2, slot, account2.clone());
        assert_eq!(max, cache.data_size());
        assert!(accounts_equal(&cache.load(key1, slot).unwrap(), &account1));
        assert!(accounts_equal(&cache.load(key2, slot).unwrap(), &account2));
        assert_eq!(2, cache.cache_len());
        cache.store(key2, slot, account1.clone()); // overwrite key2 with account1
        assert_eq!(max, cache.data_size());
        assert!(accounts_equal(&cache.load(key1, slot).unwrap(), &account1));
        assert!(accounts_equal(&cache.load(key2, slot).unwrap(), &account1));
        assert_eq!(2, cache.cache_len());
        cache.store(key3, slot, account3.clone());
        assert_eq!(max, cache.data_size());
        assert!(cache.load(key1, slot).is_none()); // was lru purged
        assert!(accounts_equal(&cache.load(key2, slot).unwrap(), &account1));
        assert!(accounts_equal(&cache.load(key3, slot).unwrap(), &account3));
        assert_eq!(2, cache.cache_len());
    }

    #[test]
    fn test_read_only_accounts_cache_random() {
        const SEED: [u8; 32] = [0xdb; 32];
        const DATA_SIZE: usize = 19;
        const MAX_CACHE_SIZE: usize = 17 * (CACHE_ENTRY_SIZE + DATA_SIZE);
        let mut rng = ChaChaRng::from_seed(SEED);
        let cache = ReadOnlyAccountsCache::new(MAX_CACHE_SIZE);
        let slots: Vec<Slot> = repeat_with(|| rng.gen_range(0, 1000)).take(5).collect();
        let pubkeys: Vec<Pubkey> = repeat_with(|| {
            let mut arr = [0u8; 32];
            rng.fill(&mut arr[..]);
            Pubkey::new_from_array(arr)
        })
        .take(7)
        .collect();
        let mut hash_map = HashMap::<ReadOnlyCacheKey, (AccountSharedData, usize)>::new();
        for ix in 0..1000 {
            if rng.gen_bool(0.1) {
                let key = *cache.cache.iter().choose(&mut rng).unwrap().key();
                let (pubkey, slot) = key;
                let account = cache.load(pubkey, slot).unwrap();
                let (other, index) = hash_map.get_mut(&key).unwrap();
                assert_eq!(account, *other);
                *index = ix;
            } else {
                let mut data = vec![0u8; DATA_SIZE];
                rng.fill(&mut data[..]);
                let account = AccountSharedData::from(Account {
                    lamports: rng.gen(),
                    data,
                    executable: rng.gen(),
                    rent_epoch: rng.gen(),
                    owner: Pubkey::default(),
                });
                let slot = *slots.choose(&mut rng).unwrap();
                let pubkey = *pubkeys.choose(&mut rng).unwrap();
                let key = (pubkey, slot);
                hash_map.insert(key, (account.clone(), ix));
                cache.store(pubkey, slot, account);
            }
        }
        assert_eq!(cache.cache_len(), 17);
        assert_eq!(hash_map.len(), 35);
        let index = hash_map
            .iter()
            .filter(|(k, _)| cache.cache.contains_key(k))
            .map(|(_, (_, ix))| *ix)
            .min()
            .unwrap();
        for (key, (account, ix)) in hash_map {
            let (pubkey, slot) = key;
            assert_eq!(
                cache.load(pubkey, slot),
                if ix < index { None } else { Some(account) }
            );
        }
    }
}
