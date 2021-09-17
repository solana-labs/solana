use crate::accounts_index::{
    AccountMapEntry, AccountMapEntryInner, IndexValue, SlotList, WriteAccountMapEntry,
};
use crate::bucket_map_holder::BucketMapHolder;
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use solana_measure::measure::Measure;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::collections::{hash_map::Entry, HashMap};
use std::ops::{Bound, RangeBounds, RangeInclusive};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use std::fmt::Debug;
type K = Pubkey;
type CacheRangesHeld = RwLock<Vec<Option<RangeInclusive<Pubkey>>>>;
pub type SlotT<T> = (Slot, T);

// one instance of this represents one bin of the accounts index.
pub struct InMemAccountsIndex<T: IndexValue> {
    // backing store
    map_internal: RwLock<HashMap<Pubkey, AccountMapEntry<T>>>,
    storage: Arc<BucketMapHolder<T>>,
    bin: usize,

    // pubkey ranges that this bin must hold in the cache while the range is present in this vec
    pub(crate) cache_ranges_held: CacheRangesHeld,
    // true while ranges are being manipulated. Used to keep an async flush from removing things while a range is being held.
    stop_flush: AtomicU64,
}

impl<T: IndexValue> Debug for InMemAccountsIndex<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl<T: IndexValue> InMemAccountsIndex<T> {
    pub fn new(storage: &Arc<BucketMapHolder<T>>, bin: usize) -> Self {
        Self {
            map_internal: RwLock::default(),
            storage: Arc::clone(storage),
            bin,
            cache_ranges_held: CacheRangesHeld::default(),
            stop_flush: AtomicU64::default(),
        }
    }

    fn map(&self) -> &RwLock<HashMap<Pubkey, AccountMapEntry<T>>> {
        &self.map_internal
    }

    pub fn items<R>(&self, range: &Option<&R>) -> Vec<(K, AccountMapEntry<T>)>
    where
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        Self::update_stat(&self.stats().items, 1);
        let map = self.map().read().unwrap();
        let mut result = Vec::with_capacity(map.len());
        map.iter().for_each(|(k, v)| {
            if range.map(|range| range.contains(k)).unwrap_or(true) {
                result.push((*k, v.clone()));
            }
        });
        result
    }

    pub fn keys(&self) -> Vec<Pubkey> {
        Self::update_stat(&self.stats().keys, 1);
        self.map().read().unwrap().keys().cloned().collect()
    }

    pub fn get(&self, key: &K) -> Option<AccountMapEntry<T>> {
        let m = Measure::start("get");
        let result = self.map().read().unwrap().get(key).cloned();
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
    pub fn remove_if_slot_list_empty(&self, pubkey: Pubkey) -> bool {
        let m = Measure::start("entry");
        let mut map = self.map().write().unwrap();
        let entry = map.entry(pubkey);
        let stats = &self.stats();
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
        &self,
        pubkey: &Pubkey,
        new_value: AccountMapEntry<T>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) {
        let m = Measure::start("entry");
        let mut map = self.map().write().unwrap();
        let entry = map.entry(*pubkey);
        let stats = &self.stats();
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
        new_value.set_dirty(true);
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
        if let Some(current) = self.map().read().unwrap().get(pubkey) {
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
        self.map().read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // return None if item was created new
    // if entry for pubkey already existed, return Some(entry). Caller needs to call entry.update.
    pub fn insert_new_entry_if_missing_with_lock(
        &self,
        pubkey: Pubkey,
        new_entry: AccountMapEntry<T>,
    ) -> Option<(WriteAccountMapEntry<T>, T, Pubkey)> {
        let m = Measure::start("entry");
        let mut map = self.map().write().unwrap();
        let entry = map.entry(pubkey);
        let stats = &self.stats();
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

    pub fn just_set_hold_range_in_memory<R>(&self, range: &R, start_holding: bool)
    where
        R: RangeBounds<Pubkey>,
    {
        let start = match range.start_bound() {
            Bound::Included(bound) | Bound::Excluded(bound) => *bound,
            Bound::Unbounded => Pubkey::new(&[0; 32]),
        };

        let end = match range.end_bound() {
            Bound::Included(bound) | Bound::Excluded(bound) => *bound,
            Bound::Unbounded => Pubkey::new(&[0xff; 32]),
        };

        // this becomes inclusive - that is ok - we are just roughly holding a range of items.
        // inclusive is bigger than exclusive so we may hold 1 extra item worst case
        let inclusive_range = Some(start..=end);
        let mut ranges = self.cache_ranges_held.write().unwrap();
        if start_holding {
            ranges.push(inclusive_range);
        } else {
            // find the matching range and delete it since we don't want to hold it anymore
            let none = inclusive_range.is_none();
            for (i, r) in ranges.iter().enumerate() {
                if r.is_none() != none {
                    continue;
                }
                if !none {
                    // neither are none, so check values
                    if let (Bound::Included(start_found), Bound::Included(end_found)) = r
                        .as_ref()
                        .map(|r| (r.start_bound(), r.end_bound()))
                        .unwrap()
                    {
                        if start_found != &start || end_found != &end {
                            continue;
                        }
                    }
                }
                // found a match. There may be dups, that's ok, we expect another call to remove the dup.
                ranges.remove(i);
                break;
            }
        }
    }

    fn start_stop_flush(&self, stop: bool) {
        if stop {
            self.stop_flush.fetch_add(1, Ordering::Acquire);
        } else {
            self.stop_flush.fetch_sub(1, Ordering::Release);
        }
    }

    pub fn hold_range_in_memory<R>(&self, range: &R, start_holding: bool)
    where
        R: RangeBounds<Pubkey> + Debug,
    {
        self.start_stop_flush(true);

        if start_holding {
            // put everything in the cache and it will be held there
            self.put_range_in_cache(Some(range));
        }
        // do this AFTER items have been put in cache - that way anyone who finds this range can know that the items are already in the cache
        self.just_set_hold_range_in_memory(range, start_holding);

        self.start_stop_flush(false);
    }

    fn put_range_in_cache<R>(&self, _range: Option<&R>)
    where
        R: RangeBounds<Pubkey>,
    {
        assert!(self.get_stop_flush()); // caller should be controlling the lifetime of how long this needs to be present
        let m = Measure::start("range");

        // load from disk here
        Self::update_time_stat(&self.stats().get_range_us, m);
    }

    fn get_stop_flush(&self) -> bool {
        self.stop_flush.load(Ordering::Relaxed) > 0
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts_index::BINS_FOR_TESTING;

    fn new_for_test<T: IndexValue>() -> InMemAccountsIndex<T> {
        let holder = Arc::new(BucketMapHolder::new(BINS_FOR_TESTING));
        InMemAccountsIndex::new(&holder, BINS_FOR_TESTING)
    }

    #[test]
    fn test_hold_range_in_memory() {
        let accts = new_for_test::<u64>();
        // 0x81 is just some other range
        let ranges = [
            Pubkey::new(&[0; 32])..=Pubkey::new(&[0xff; 32]),
            Pubkey::new(&[0x81; 32])..=Pubkey::new(&[0xff; 32]),
        ];
        for range in ranges.clone() {
            assert!(accts.cache_ranges_held.read().unwrap().is_empty());
            accts.hold_range_in_memory(&range, true);
            assert_eq!(
                accts.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone())]
            );
            accts.hold_range_in_memory(&range, false);
            assert!(accts.cache_ranges_held.read().unwrap().is_empty());
            accts.hold_range_in_memory(&range, true);
            assert_eq!(
                accts.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone())]
            );
            accts.hold_range_in_memory(&range, true);
            assert_eq!(
                accts.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone()), Some(range.clone())]
            );
            accts.hold_range_in_memory(&ranges[0], true);
            assert_eq!(
                accts.cache_ranges_held.read().unwrap().to_vec(),
                vec![
                    Some(range.clone()),
                    Some(range.clone()),
                    Some(ranges[0].clone())
                ]
            );
            accts.hold_range_in_memory(&range, false);
            assert_eq!(
                accts.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone()), Some(ranges[0].clone())]
            );
            accts.hold_range_in_memory(&range, false);
            assert_eq!(
                accts.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(ranges[0].clone())]
            );
            accts.hold_range_in_memory(&ranges[0].clone(), false);
            assert!(accts.cache_ranges_held.read().unwrap().is_empty());
        }
    }
}
