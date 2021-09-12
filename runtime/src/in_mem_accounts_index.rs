use crate::accounts_index::{AccountMapEntry, IsCached, WriteAccountMapEntry};
use crate::accounts_index_storage::AccountsIndexStorage;
use crate::bucket_map_holder::BucketMapHolder;
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use solana_measure::measure::Measure;
use solana_sdk::pubkey::Pubkey;
use std::collections::{
    hash_map::{Entry, Keys},
    HashMap,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use std::fmt::Debug;

type K = Pubkey;

// one instance of this represents one bin of the accounts index.
#[derive(Debug)]
pub struct InMemAccountsIndex<T: IsCached> {
    // backing store
    map: HashMap<Pubkey, AccountMapEntry<T>>,
    storage: Arc<BucketMapHolder>,
}

impl<T: IsCached> InMemAccountsIndex<T> {
    pub fn new(storage: &AccountsIndexStorage) -> Self {
        Self {
            map: HashMap::new(),
            storage: storage.storage().clone(),
        }
    }

    pub fn new_bucket_map_holder() -> Arc<BucketMapHolder> {
        Arc::new(BucketMapHolder::new())
    }

    pub fn entry(&mut self, pubkey: Pubkey) -> Entry<K, AccountMapEntry<T>> {
        let m = Measure::start("entry");
        let result = self.map.entry(pubkey);
        let stats = &self.storage.stats;
        let (count, time) = if matches!(result, Entry::Occupied(_)) {
            (&stats.gets_from_mem, &stats.get_mem_us)
        } else {
            (&stats.gets_missing, &stats.get_missing_us)
        };
        Self::update_time_stat(time, m);
        Self::update_stat(count, 1);
        result
    }

    pub fn items(&self) -> Vec<(K, AccountMapEntry<T>)> {
        Self::update_stat(&self.stats().items, 1);
        self.map.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    pub fn keys(&self) -> Keys<K, AccountMapEntry<T>> {
        Self::update_stat(&self.stats().keys, 1);
        self.map.keys()
    }

    pub fn get(&self, key: &K) -> Option<AccountMapEntry<T>> {
        let m = Measure::start("get");
        let result = self.map.get(key).cloned();
        let stats = self.stats();
        let (count, time) = if result.is_some() {
            (&stats.gets_from_mem, &stats.get_mem_us)
        } else {
            (&stats.gets_missing, &stats.get_missing_us)
        };
        Self::update_time_stat(time, m);
        Self::update_stat(count, 1);
        result
    }

    pub fn remove(&mut self, key: &K) {
        Self::update_stat(&self.stats().deletes, 1);
        self.map.remove(key);
    }

    // If the slot list for pubkey exists in the index and is empty, remove the index entry for pubkey and return true.
    // Return false otherwise.
    pub fn remove_if_slot_list_empty(&mut self, pubkey: Pubkey) -> bool {
        if let Entry::Occupied(index_entry) = self.map.entry(pubkey) {
            if index_entry.get().slot_list.read().unwrap().is_empty() {
                index_entry.remove();
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // return None if item was created new
    // if entry for pubkey already existed, return Some(entry). Caller needs to call entry.update.
    pub fn insert_new_entry_if_missing_with_lock(
        &mut self,
        pubkey: Pubkey,
        new_entry: AccountMapEntry<T>,
    ) -> Option<(WriteAccountMapEntry<T>, T, Pubkey)> {
        let account_entry = self.map.entry(pubkey);
        match account_entry {
            Entry::Occupied(account_entry) => Some((
                WriteAccountMapEntry::from_account_map_entry(account_entry.get().clone()),
                // extract the new account_info from the unused 'new_entry'
                new_entry.slot_list.write().unwrap().remove(0).1,
                *account_entry.key(),
            )),
            Entry::Vacant(account_entry) => {
                account_entry.insert(new_entry);
                None
            }
        }
    }

    fn stats(&self) -> &BucketMapHolderStats {
        &self.storage.stats
    }

    fn update_stat(stat: &AtomicU64, value: u64) {
        if value != 0 {
            stat.fetch_add(value, Ordering::Relaxed);
        }
    }

    pub fn update_time_stat(stat: &AtomicU64, mut m: Measure) {
        m.stop();
        let value = m.as_us();
        Self::update_stat(stat, value);
    }
}
