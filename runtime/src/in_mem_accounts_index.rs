use crate::accounts_index::{
    AccountMapEntry, AccountMapEntryInner, IndexValue, SlotList, WriteAccountMapEntry,
};
use crate::accounts_index_storage::AccountsIndexStorage;
use crate::bucket_map_holder::BucketMapHolder;
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use solana_measure::measure::Measure;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::collections::{hash_map::Entry, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use std::fmt::Debug;
use std::ops::RangeBounds;
type K = Pubkey;

// one instance of this represents one bin of the accounts index.
#[derive(Debug)]
pub struct InMemAccountsIndex<T: IndexValue> {
    // backing store
    map: HashMap<Pubkey, AccountMapEntry<T>>,
    storage: Arc<BucketMapHolder<T>>,
    bin: usize,
}

impl<T: IndexValue> InMemAccountsIndex<T> {
    pub fn new(storage: &AccountsIndexStorage<T>, bin: usize) -> Self {
        Self {
            map: HashMap::new(),
            storage: storage.storage().clone(),
            bin,
        }
    }

    pub fn new_bucket_map_holder() -> Arc<BucketMapHolder<T>> {
        Arc::new(BucketMapHolder::new())
    }

    pub fn items<R>(&self, range: &Option<&R>) -> Vec<(K, AccountMapEntry<T>)>
    where
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        Self::update_stat(&self.stats().items, 1);
        let mut result = Vec::with_capacity(self.map.len());
        self.map.iter().for_each(|(k, v)| {
            if range.map(|range| range.contains(k)).unwrap_or(true) {
                result.push((*k, v.clone()));
            }
        });
        result
    }

    pub fn keys(&self) -> Vec<Pubkey> {
        Self::update_stat(&self.stats().keys, 1);
        self.map.keys().cloned().collect()
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

    // If the slot list for pubkey exists in the index and is empty, remove the index entry for pubkey and return true.
    // Return false otherwise.
    pub fn remove_if_slot_list_empty(&mut self, pubkey: Pubkey) -> bool {
        let m = Measure::start("entry");
        let entry = self.map.entry(pubkey);
        let stats = &self.storage.stats;
        let (count, time) = if matches!(entry, Entry::Occupied(_)) {
            (&stats.entries_from_mem, &stats.entry_mem_us)
        } else {
            (&stats.entries_missing, &stats.entry_missing_us)
        };
        Self::update_time_stat(time, m);
        Self::update_stat(count, 1);

        if let Entry::Occupied(index_entry) = entry {
            if index_entry.get().slot_list.read().unwrap().is_empty() {
                index_entry.remove();
                self.stats().insert_or_delete(false, self.bin);
                return true;
            }
        }
        false
    }

    pub fn upsert(
        &mut self,
        pubkey: &Pubkey,
        new_value: AccountMapEntry<T>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) {
        let m = Measure::start("entry");
        let entry = self.map.entry(*pubkey);
        let stats = &self.storage.stats;
        let (count, time) = if matches!(entry, Entry::Occupied(_)) {
            (&stats.entries_from_mem, &stats.entry_mem_us)
        } else {
            (&stats.entries_missing, &stats.entry_missing_us)
        };
        Self::update_time_stat(time, m);
        Self::update_stat(count, 1);
        match entry {
            Entry::Occupied(mut occupied) => {
                let current = occupied.get_mut();
                Self::lock_and_update_slot_list(
                    current,
                    &new_value,
                    reclaims,
                    previous_slot_entry_was_cached,
                );
                Self::update_stat(&self.stats().updates_in_mem, 1);
            }
            Entry::Vacant(vacant) => {
                vacant.insert(new_value);
                self.stats().insert_or_delete(true, self.bin);
            }
        }
    }

    pub fn lock_and_update_slot_list(
        current: &Arc<AccountMapEntryInner<T>>,
        new_value: &AccountMapEntry<T>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) {
        let mut slot_list = current.slot_list.write().unwrap();
        let (slot, new_entry) = new_value.slot_list.write().unwrap().remove(0);
        let addref = Self::update_slot_list(
            &mut slot_list,
            slot,
            new_entry,
            reclaims,
            previous_slot_entry_was_cached,
        );
        if addref {
            current.add_un_ref(true);
        }
    }

    // modifies slot_list
    // returns true if caller should addref
    pub fn update_slot_list(
        list: &mut SlotList<T>,
        slot: Slot,
        account_info: T,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) -> bool {
        let mut addref = !account_info.is_cached();

        // find other dirty entries from the same slot
        for list_index in 0..list.len() {
            let (s, previous_update_value) = &list[list_index];
            if *s == slot {
                let previous_was_cached = previous_update_value.is_cached();
                addref = addref && previous_was_cached;

                let mut new_item = (slot, account_info);
                std::mem::swap(&mut new_item, &mut list[list_index]);
                if previous_slot_entry_was_cached {
                    assert!(previous_was_cached);
                } else {
                    reclaims.push(new_item);
                }
                list[(list_index + 1)..]
                    .iter()
                    .for_each(|item| assert!(item.0 != slot));
                return addref;
            }
        }

        // if we make it here, we did not find the slot in the list
        list.push((slot, account_info));
        addref
    }

    // returns true if upsert was successful. new_value is modified in this case. new_value contains a RwLock
    // otherwise, new_value has not been modified and the pubkey has to be added to the maps with a write lock. call upsert_new
    pub fn update_key_if_exists(
        &self,
        pubkey: &Pubkey,
        new_value: &AccountMapEntry<T>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) -> bool {
        if let Some(current) = self.map.get(pubkey) {
            Self::lock_and_update_slot_list(
                current,
                new_value,
                reclaims,
                previous_slot_entry_was_cached,
            );
            true
        } else {
            false
        }
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
        let m = Measure::start("entry");
        let entry = self.map.entry(pubkey);
        let stats = &self.storage.stats;
        let (count, time) = if matches!(entry, Entry::Occupied(_)) {
            (&stats.entries_from_mem, &stats.entry_mem_us)
        } else {
            (&stats.entries_missing, &stats.entry_missing_us)
        };
        Self::update_time_stat(time, m);
        Self::update_stat(count, 1);
        let result = match entry {
            Entry::Occupied(account_entry) => {
                Some((
                    WriteAccountMapEntry::from_account_map_entry(account_entry.get().clone()),
                    // extract the new account_info from the unused 'new_entry'
                    new_entry.slot_list.write().unwrap().remove(0).1,
                    *account_entry.key(),
                ))
            }
            Entry::Vacant(account_entry) => {
                account_entry.insert(new_entry);
                None
            }
        };
        let stats = self.stats();
        if result.is_none() {
            stats.insert_or_delete(true, self.bin);
        } else {
            Self::update_stat(&stats.updates_in_mem, 1);
        }
        result
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
