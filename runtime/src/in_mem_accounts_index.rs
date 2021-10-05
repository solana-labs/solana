use crate::accounts_index::{
    AccountMapEntry, AccountMapEntryInner, AccountMapEntryMeta, IndexValue,
    PreAllocatedAccountMapEntry, RefCount, SlotList, SlotSlice,
};
use crate::bucket_map_holder::{Age, BucketMapHolder};
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use rand::thread_rng;
use rand::Rng;
use solana_bucket_map::bucket_api::BucketApi;
use solana_measure::measure::Measure;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::collections::{hash_map::Entry, HashMap};
use std::ops::{Bound, RangeBounds, RangeInclusive};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use std::fmt::Debug;
type K = Pubkey;
type CacheRangesHeld = RwLock<Vec<Option<RangeInclusive<Pubkey>>>>;
pub type SlotT<T> = (Slot, T);

#[allow(dead_code)] // temporary during staging
                    // one instance of this represents one bin of the accounts index.
pub struct InMemAccountsIndex<T: IndexValue> {
    last_age_flushed: AtomicU8,

    // backing store
    map_internal: RwLock<HashMap<Pubkey, AccountMapEntry<T>>>,
    storage: Arc<BucketMapHolder<T>>,
    bin: usize,

    bucket: Option<Arc<BucketApi<SlotT<T>>>>,

    // pubkey ranges that this bin must hold in the cache while the range is present in this vec
    pub(crate) cache_ranges_held: CacheRangesHeld,
    // true while ranges are being manipulated. Used to keep an async flush from removing things while a range is being held.
    stop_flush: AtomicU64,
    // set to true when any entry in this bin is marked dirty
    bin_dirty: AtomicBool,
    // set to true while this bin is being actively flushed
    flushing_active: AtomicBool,
}

impl<T: IndexValue> Debug for InMemAccountsIndex<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

#[allow(dead_code)] // temporary during staging
impl<T: IndexValue> InMemAccountsIndex<T> {
    pub fn new(storage: &Arc<BucketMapHolder<T>>, bin: usize) -> Self {
        Self {
            map_internal: RwLock::default(),
            storage: Arc::clone(storage),
            bin,
            bucket: storage
                .disk
                .as_ref()
                .map(|disk| disk.get_bucket_from_index(bin))
                .map(Arc::clone),
            cache_ranges_held: CacheRangesHeld::default(),
            stop_flush: AtomicU64::default(),
            bin_dirty: AtomicBool::default(),
            flushing_active: AtomicBool::default(),
            // initialize this to max, to make it clear we have not flushed at age 0, the starting age
            last_age_flushed: AtomicU8::new(Age::MAX),
        }
    }

    /// true if this bucket needs to call flush for the current age
    /// we need to scan each bucket once per value of age
    fn get_should_age(&self, age: Age) -> bool {
        let last_age_flushed = self.last_age_flushed();
        last_age_flushed != age
    }

    /// called after flush scans this bucket at the current age
    fn set_has_aged(&self, age: Age) {
        self.last_age_flushed.store(age, Ordering::Relaxed);
        self.storage.bucket_flushed_at_current_age();
    }

    fn last_age_flushed(&self) -> Age {
        self.last_age_flushed.load(Ordering::Relaxed)
    }

    fn map(&self) -> &RwLock<HashMap<Pubkey, AccountMapEntry<T>>> {
        &self.map_internal
    }

    pub fn items<R>(&self, range: &Option<&R>) -> Vec<(K, AccountMapEntry<T>)>
    where
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        self.start_stop_flush(true);
        self.put_range_in_cache(range); // check range here to see if our items are already held in the cache
        Self::update_stat(&self.stats().items, 1);
        let map = self.map().read().unwrap();
        let mut result = Vec::with_capacity(map.len());
        map.iter().for_each(|(k, v)| {
            if range.map(|range| range.contains(k)).unwrap_or(true) {
                result.push((*k, Arc::clone(v)));
            }
        });
        self.start_stop_flush(false);
        result
    }

    // only called in debug code paths
    pub fn keys(&self) -> Vec<Pubkey> {
        Self::update_stat(&self.stats().keys, 1);
        // easiest implementation is to load evrything from disk into cache and return the keys
        self.start_stop_flush(true);
        self.put_range_in_cache(&None::<&RangeInclusive<Pubkey>>);
        let keys = self.map().read().unwrap().keys().cloned().collect();
        self.start_stop_flush(false);
        keys
    }

    fn load_from_disk(&self, pubkey: &Pubkey) -> Option<(SlotList<T>, RefCount)> {
        self.bucket.as_ref().and_then(|disk| {
            let m = Measure::start("load_disk_found_count");
            let entry_disk = disk.read_value(pubkey);
            match &entry_disk {
                Some(_) => {
                    Self::update_time_stat(&self.stats().load_disk_found_us, m);
                    Self::update_stat(&self.stats().load_disk_found_count, 1);
                }
                None => {
                    Self::update_time_stat(&self.stats().load_disk_missing_us, m);
                    Self::update_stat(&self.stats().load_disk_missing_count, 1);
                }
            }
            entry_disk
        })
    }

    fn load_account_entry_from_disk(&self, pubkey: &Pubkey) -> Option<AccountMapEntry<T>> {
        let entry_disk = self.load_from_disk(pubkey)?; // returns None if not on disk

        Some(self.disk_to_cache_entry(entry_disk.0, entry_disk.1))
    }

    /// lookup 'pubkey' by only looking in memory. Does not look on disk.
    /// callback is called whether pubkey is found or not
    fn get_only_in_mem<RT>(
        &self,
        pubkey: &K,
        callback: impl for<'a> FnOnce(Option<&'a Arc<AccountMapEntryInner<T>>>) -> RT,
    ) -> RT {
        let m = Measure::start("get");
        let map = self.map().read().unwrap();
        let result = map.get(pubkey);
        let stats = self.stats();
        let (count, time) = if result.is_some() {
            (&stats.gets_from_mem, &stats.get_mem_us)
        } else {
            (&stats.gets_missing, &stats.get_missing_us)
        };
        Self::update_time_stat(time, m);
        Self::update_stat(count, 1);

        callback(if let Some(entry) = result {
            entry.set_age(self.storage.future_age_to_flush());
            Some(entry)
        } else {
            drop(map);
            None
        })
    }

    /// lookup 'pubkey' in index (in mem or on disk)
    pub fn get(&self, pubkey: &K) -> Option<AccountMapEntry<T>> {
        self.get_internal(pubkey, |entry| entry.map(Arc::clone))
    }

    /// lookup 'pubkey' in index.
    /// call 'callback' whether found or not
    fn get_internal<RT>(
        &self,
        pubkey: &K,
        callback: impl for<'a> FnOnce(Option<&Arc<AccountMapEntryInner<T>>>) -> RT,
    ) -> RT {
        self.get_only_in_mem(pubkey, |entry| {
            if let Some(entry) = entry {
                entry.set_age(self.storage.future_age_to_flush());
                callback(Some(entry))
            } else {
                // not in cache, look on disk
                let stats = &self.stats();
                let disk_entry = self.load_account_entry_from_disk(pubkey);
                if disk_entry.is_none() {
                    return callback(None);
                }
                let disk_entry = disk_entry.unwrap();
                let mut map = self.map().write().unwrap();
                let entry = map.entry(*pubkey);
                match entry {
                    Entry::Occupied(occupied) => callback(Some(occupied.get())),
                    Entry::Vacant(vacant) => {
                        stats.insert_or_delete_mem(true, self.bin);
                        callback(Some(vacant.insert(disk_entry)))
                    }
                }
            }
        })
    }

    fn remove_if_slot_list_empty_value(&self, slot_list: SlotSlice<T>) -> bool {
        if slot_list.is_empty() {
            self.stats().insert_or_delete(false, self.bin);
            true
        } else {
            false
        }
    }

    fn delete_disk_key(&self, pubkey: &Pubkey) {
        if let Some(disk) = self.bucket.as_ref() {
            disk.delete_key(pubkey)
        }
    }

    fn remove_if_slot_list_empty_entry(&self, entry: Entry<K, AccountMapEntry<T>>) -> bool {
        match entry {
            Entry::Occupied(occupied) => {
                let result =
                    self.remove_if_slot_list_empty_value(&occupied.get().slot_list.read().unwrap());
                if result {
                    // note there is a potential race here that has existed.
                    // if someone else holds the arc,
                    //  then they think the item is still in the index and can make modifications.
                    // We have to have a write lock to the map here, which means nobody else can get
                    //  the arc, but someone may already have retreived a clone of it.
                    // account index in_mem flushing is one such possibility
                    self.delete_disk_key(occupied.key());
                    self.stats().insert_or_delete_mem(false, self.bin);
                    occupied.remove();
                }
                result
            }
            Entry::Vacant(vacant) => {
                // not in cache, look on disk
                let entry_disk = self.load_from_disk(vacant.key());
                match entry_disk {
                    Some(entry_disk) => {
                        // on disk
                        if self.remove_if_slot_list_empty_value(&entry_disk.0) {
                            // not in cache, but on disk, so just delete from disk
                            self.delete_disk_key(vacant.key());
                            true
                        } else {
                            // could insert into cache here, but not required for correctness and value is unclear
                            false
                        }
                    }
                    None => false, // not in cache or on disk
                }
            }
        }
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

        self.remove_if_slot_list_empty_entry(entry)
    }

    pub fn slot_list_mut<RT>(
        &self,
        pubkey: &Pubkey,
        user: impl for<'a> FnOnce(&mut RwLockWriteGuard<'a, SlotList<T>>) -> RT,
    ) -> Option<RT> {
        self.get_internal(pubkey, |entry| {
            entry.map(|entry| {
                let result = user(&mut entry.slot_list.write().unwrap());
                entry.set_dirty(true);
                result
            })
        })
    }

    pub fn upsert(
        &self,
        pubkey: &Pubkey,
        new_value: PreAllocatedAccountMapEntry<T>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) {
        // try to get it just from memory first using only a read lock
        self.get_only_in_mem(pubkey, |entry| {
            if let Some(entry) = entry {
                Self::lock_and_update_slot_list(
                    entry,
                    new_value.into(),
                    reclaims,
                    previous_slot_entry_was_cached,
                );
                Self::update_stat(&self.stats().updates_in_mem, 1);
            } else {
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
                            new_value.into(),
                            reclaims,
                            previous_slot_entry_was_cached,
                        );
                        current.set_age(self.storage.future_age_to_flush());
                        Self::update_stat(&self.stats().updates_in_mem, 1);
                    }
                    Entry::Vacant(vacant) => {
                        // not in cache, look on disk
                        let disk_entry = self.load_account_entry_from_disk(vacant.key());
                        let new_value = if let Some(disk_entry) = disk_entry {
                            // on disk, so merge new_value with what was on disk
                            Self::lock_and_update_slot_list(
                                &disk_entry,
                                new_value.into(),
                                reclaims,
                                previous_slot_entry_was_cached,
                            );
                            disk_entry
                        } else {
                            // not on disk, so insert new thing
                            new_value.into()
                        };
                        assert!(new_value.dirty());
                        vacant.insert(new_value);
                        self.stats().insert_or_delete_mem(true, self.bin);
                        self.stats().insert_or_delete(true, self.bin);
                    }
                }
            };
        })
    }

    // Try to update an item in the slot list the given `slot` If an item for the slot
    // already exists in the list, remove the older item, add it to `reclaims`, and insert
    // the new item.
    pub fn lock_and_update_slot_list(
        current: &AccountMapEntryInner<T>,
        new_value: (Slot, T),
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) {
        let mut slot_list = current.slot_list.write().unwrap();
        let (slot, new_entry) = new_value;
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
        current.set_dirty(true);
    }

    // modifies slot_list
    // returns true if caller should addref
    fn update_slot_list(
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

    // convert from raw data on disk to AccountMapEntry, set to age in future
    fn disk_to_cache_entry(
        &self,
        slot_list: SlotList<T>,
        ref_count: RefCount,
    ) -> AccountMapEntry<T> {
        Arc::new(AccountMapEntryInner::new(
            slot_list,
            ref_count,
            AccountMapEntryMeta::new_dirty(&self.storage),
        ))
    }

    pub fn len(&self) -> usize {
        self.map().read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn insert_returner(
        existing: &AccountMapEntry<T>,
        pubkey: &Pubkey,
        new_entry: PreAllocatedAccountMapEntry<T>,
    ) -> (AccountMapEntry<T>, T, Pubkey) {
        let (_slot, info): (Slot, T) = new_entry.into();
        (
            Arc::clone(existing),
            // extract the new account_info from the unused 'new_entry'
            info,
            *pubkey,
        )
    }

    // return None if item was created new
    // if entry for pubkey already existed, return Some(entry). Caller needs to call entry.update.
    pub fn insert_new_entry_if_missing_with_lock(
        &self,
        pubkey: Pubkey,
        new_entry: PreAllocatedAccountMapEntry<T>,
    ) -> Option<(AccountMapEntry<T>, T, Pubkey)> {
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
            Entry::Occupied(occupied) => Some(Self::insert_returner(
                occupied.get(),
                occupied.key(),
                new_entry,
            )),
            Entry::Vacant(vacant) => {
                // not in cache, look on disk
                let disk_entry = self.load_account_entry_from_disk(vacant.key());
                stats.insert_or_delete_mem(true, self.bin);
                if let Some(disk_entry) = disk_entry {
                    // on disk, so insert into cache, then return cache value so caller will merge
                    let result = Some(Self::insert_returner(&disk_entry, vacant.key(), new_entry));
                    assert!(disk_entry.dirty());
                    vacant.insert(disk_entry);
                    result
                } else {
                    // not on disk, so insert new thing and we're done
                    let new_entry: AccountMapEntry<T> = new_entry.into();
                    assert!(new_entry.dirty());
                    vacant.insert(new_entry);
                    None // returns None if item was created new
                }
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
            self.stop_flush.fetch_add(1, Ordering::Release);
        } else if 1 == self.stop_flush.fetch_sub(1, Ordering::Release) {
            // stop_flush went to 0, so this bucket could now be ready to be aged
            self.storage.wait_dirty_or_aged.notify_one();
        }
    }

    pub fn hold_range_in_memory<R>(&self, range: &R, start_holding: bool)
    where
        R: RangeBounds<Pubkey> + Debug,
    {
        self.start_stop_flush(true);

        if start_holding {
            // put everything in the cache and it will be held there
            self.put_range_in_cache(&Some(range));
        }
        // do this AFTER items have been put in cache - that way anyone who finds this range can know that the items are already in the cache
        self.just_set_hold_range_in_memory(range, start_holding);

        self.start_stop_flush(false);
    }

    fn put_range_in_cache<R>(&self, range: &Option<&R>)
    where
        R: RangeBounds<Pubkey>,
    {
        assert!(self.get_stop_flush()); // caller should be controlling the lifetime of how long this needs to be present
        let m = Measure::start("range");

        // load from disk
        if let Some(disk) = self.bucket.as_ref() {
            let items = disk.items_in_range(range);
            let mut map = self.map().write().unwrap();
            let future_age = self.storage.future_age_to_flush();
            for item in items {
                let entry = map.entry(item.pubkey);
                match entry {
                    Entry::Occupied(occupied) => {
                        // item already in cache, bump age to future. This helps the current age flush to succeed.
                        occupied.get().set_age(future_age);
                    }
                    Entry::Vacant(vacant) => {
                        vacant.insert(self.disk_to_cache_entry(item.slot_list, item.ref_count));
                        self.stats().insert_or_delete_mem(true, self.bin);
                    }
                }
            }
        }

        Self::update_time_stat(&self.stats().get_range_us, m);
    }

    fn get_stop_flush(&self) -> bool {
        self.stop_flush.load(Ordering::Relaxed) > 0
    }

    pub(crate) fn flush(&self) {
        let flushing = self.flushing_active.swap(true, Ordering::Acquire);
        if flushing {
            // already flushing in another thread
            return;
        }

        self.flush_internal();

        self.flushing_active.store(false, Ordering::Release);
    }

    pub fn set_bin_dirty(&self) {
        self.bin_dirty.store(true, Ordering::Release);
        // 1 bin dirty, so only need 1 thread to wake up if many could be waiting
        self.storage.wait_dirty_or_aged.notify_one();
    }

    fn random_chance_of_eviction() -> bool {
        // random eviction
        const N: usize = 1000;
        // 1/N chance of eviction
        thread_rng().gen_range(0, N) == 0
    }

    /// return true if 'entry' should be removed from the in-mem index
    fn should_remove_from_mem(
        &self,
        current_age: Age,
        entry: &AccountMapEntry<T>,
        startup: bool,
    ) -> bool {
        // this could be tunable dynamically based on memory pressure
        // we could look at more ages or we could throw out more items we are choosing to keep in the cache
        if startup || (current_age == entry.age()) {
            // only read the slot list if we are planning to throw the item out
            let slot_list = entry.slot_list.read().unwrap();
            if slot_list.len() != 1 {
                false // keep 0 and > 1 slot lists in mem. They will be cleaned or shrunk soon.
            } else {
                // keep items with slot lists that contained cached items
                !slot_list.iter().any(|(_, info)| info.is_cached())
            }
        } else {
            false
        }
    }

    fn flush_internal(&self) {
        let was_dirty = self.bin_dirty.swap(false, Ordering::Acquire);
        let current_age = self.storage.current_age();
        let mut iterate_for_age = self.get_should_age(current_age);
        let startup = self.storage.get_startup();
        if !was_dirty && !iterate_for_age && !startup {
            // wasn't dirty and no need to age, so no need to flush this bucket
            // but, at startup we want to remove from buckets as fast as possible if any items exist
            return;
        }

        // may have to loop if disk has to grow and we have to restart
        loop {
            let mut removes = Vec::default();
            let mut removes_random = Vec::default();
            let disk = self.bucket.as_ref().unwrap();

            let mut updates = Vec::default();
            let m = Measure::start("flush_scan");
            let mut flush_entries_updated_on_disk = 0;
            let mut disk_resize = Ok(());
            // scan and update loop
            {
                let map = self.map().read().unwrap();
                for (k, v) in map.iter() {
                    if v.dirty() {
                        // step 1: clear the dirty flag
                        // step 2: perform the update on disk based on the fields in the entry
                        // If a parallel operation dirties the item again - even while this flush is occurring,
                        //  the last thing the writer will do, after updating contents, is set_dirty(true)
                        //  That prevents dropping an item from cache before disk is updated to latest in mem.
                        v.set_dirty(false);

                        updates.push((*k, Arc::clone(v)));
                    }

                    if self.should_remove_from_mem(current_age, v, startup) {
                        removes.push(*k);
                    } else if Self::random_chance_of_eviction() {
                        removes_random.push(*k);
                    }
                }
                Self::update_time_stat(&self.stats().flush_scan_us, m);

                // happens inside of lock on in-mem cache. This is because of deleting items
                // it is possible that the item in the cache is marked as dirty while these updates are happening. That is ok.
                let m = Measure::start("flush_update");
                for (k, v) in updates.into_iter() {
                    if disk_resize.is_ok() {
                        if v.dirty() {
                            continue; // marked dirty after we grabbed it above, so handle this the next time this bucket is flushed
                        }
                        flush_entries_updated_on_disk += 1;
                        disk_resize =
                            disk.try_write(&k, (&v.slot_list.read().unwrap(), v.ref_count()));
                    }
                    if disk_resize.is_err() {
                        // disk needs to resize, so mark all unprocessed items as dirty again so we pick them up after the resize
                        v.set_dirty(true);
                    }
                }
                Self::update_time_stat(&self.stats().flush_update_us, m);
                Self::update_stat(
                    &self.stats().flush_entries_updated_on_disk,
                    flush_entries_updated_on_disk,
                );
            }

            let m = Measure::start("flush_remove");
            match disk_resize {
                Ok(_) => {
                    if !self.flush_remove_from_cache(removes, current_age, startup, false) {
                        iterate_for_age = false; // did not make it all the way through this bucket, so didn't handle age completely
                    }
                    if !self.flush_remove_from_cache(removes_random, current_age, startup, true) {
                        iterate_for_age = false; // did not make it all the way through this bucket, so didn't handle age completely
                    }
                    Self::update_time_stat(&self.stats().flush_remove_us, m);

                    if iterate_for_age {
                        // completed iteration of the buckets at the current age
                        assert_eq!(current_age, self.storage.current_age());
                        self.set_has_aged(current_age);
                    }
                    return;
                }
                Err(err) => {
                    // grow the bucket, outside of all in-mem locks.
                    // then, loop to try again
                    disk.grow(err);
                }
            }
        }
    }

    // remove keys in 'removes' from in-mem cache due to age
    // return true if the removal was completed
    fn flush_remove_from_cache(
        &self,
        removes: Vec<Pubkey>,
        current_age: Age,
        startup: bool,
        randomly_evicted: bool,
    ) -> bool {
        let mut completed_scan = true;
        if removes.is_empty() {
            return completed_scan; // completed, don't need to get lock or do other work
        }

        let ranges = self.cache_ranges_held.read().unwrap().clone();
        if ranges.iter().any(|range| range.is_none()) {
            return false; // range said to hold 'all', so not completed
        }
        let mut map = self.map().write().unwrap();
        for k in removes {
            if let Entry::Occupied(occupied) = map.entry(k) {
                let v = occupied.get();
                if Arc::strong_count(v) > 1 {
                    // someone is holding the value arc's ref count and could modify it, so do not remove this from in-mem cache
                    completed_scan = false;
                    continue;
                }

                if v.dirty()
                    || (!randomly_evicted && !self.should_remove_from_mem(current_age, v, startup))
                {
                    // marked dirty or bumped in age after we looked above
                    // these will be handled in later passes
                    // but, at startup, everything is ready to age out if it isn't dirty
                    continue;
                }

                if ranges.iter().any(|range| {
                    range
                        .as_ref()
                        .map(|range| range.contains(&k))
                        .unwrap_or(true) // None means 'full range', so true
                }) {
                    // this item is held in mem by range, so don't remove
                    completed_scan = false;
                    continue;
                }

                if self.get_stop_flush() {
                    return false; // did NOT complete, told to stop
                }

                // all conditions for removing succeeded, so really remove item from in-mem cache
                self.stats().insert_or_delete_mem(false, self.bin);
                occupied.remove();
            }
        }
        completed_scan
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
    use crate::accounts_index::{AccountsIndexConfig, BINS_FOR_TESTING};

    fn new_for_test<T: IndexValue>() -> InMemAccountsIndex<T> {
        let holder = Arc::new(BucketMapHolder::new(
            BINS_FOR_TESTING,
            &Some(AccountsIndexConfig::default()),
            1,
        ));
        let bin = 0;
        InMemAccountsIndex::new(&holder, bin)
    }

    #[test]
    fn test_should_remove_from_mem() {
        solana_logger::setup();
        let bucket = new_for_test::<u64>();
        let mut startup = false;
        let mut current_age = 0;
        let ref_count = 0;
        let one_element_slot_list = vec![(0, 0)];
        let one_element_slot_list_entry = Arc::new(AccountMapEntryInner::new(
            one_element_slot_list,
            ref_count,
            AccountMapEntryMeta::default(),
        ));

        // empty slot list
        assert!(!bucket.should_remove_from_mem(
            current_age,
            &Arc::new(AccountMapEntryInner::new(
                vec![],
                ref_count,
                AccountMapEntryMeta::default()
            )),
            startup
        ));
        // 1 element slot list
        assert!(bucket.should_remove_from_mem(current_age, &one_element_slot_list_entry, startup));
        // 2 element slot list
        assert!(!bucket.should_remove_from_mem(
            current_age,
            &Arc::new(AccountMapEntryInner::new(
                vec![(0, 0), (1, 1)],
                ref_count,
                AccountMapEntryMeta::default()
            )),
            startup
        ));

        {
            let bucket = new_for_test::<f64>();
            // 1 element slot list with a CACHED item - f64 acts like cached
            assert!(!bucket.should_remove_from_mem(
                current_age,
                &Arc::new(AccountMapEntryInner::new(
                    vec![(0, 0.0)],
                    ref_count,
                    AccountMapEntryMeta::default()
                )),
                startup
            ));
        }

        // 1 element slot list, age is now
        assert!(bucket.should_remove_from_mem(current_age, &one_element_slot_list_entry, startup));

        // 1 element slot list, but not current age
        current_age = 1;
        assert!(!bucket.should_remove_from_mem(current_age, &one_element_slot_list_entry, startup));

        // 1 element slot list, but at startup and age not current
        startup = true;
        assert!(bucket.should_remove_from_mem(current_age, &one_element_slot_list_entry, startup));
    }

    #[test]
    fn test_hold_range_in_memory() {
        let bucket = new_for_test::<u64>();
        // 0x81 is just some other range
        let ranges = [
            Pubkey::new(&[0; 32])..=Pubkey::new(&[0xff; 32]),
            Pubkey::new(&[0x81; 32])..=Pubkey::new(&[0xff; 32]),
        ];
        for range in ranges.clone() {
            assert!(bucket.cache_ranges_held.read().unwrap().is_empty());
            bucket.hold_range_in_memory(&range, true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone())]
            );
            bucket.hold_range_in_memory(&range, false);
            assert!(bucket.cache_ranges_held.read().unwrap().is_empty());
            bucket.hold_range_in_memory(&range, true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone())]
            );
            bucket.hold_range_in_memory(&range, true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone()), Some(range.clone())]
            );
            bucket.hold_range_in_memory(&ranges[0], true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![
                    Some(range.clone()),
                    Some(range.clone()),
                    Some(ranges[0].clone())
                ]
            );
            bucket.hold_range_in_memory(&range, false);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(range.clone()), Some(ranges[0].clone())]
            );
            bucket.hold_range_in_memory(&range, false);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![Some(ranges[0].clone())]
            );
            bucket.hold_range_in_memory(&ranges[0].clone(), false);
            assert!(bucket.cache_ranges_held.read().unwrap().is_empty());
        }
    }

    #[test]
    fn test_age() {
        solana_logger::setup();
        let test = new_for_test::<u64>();
        assert!(test.get_should_age(test.storage.current_age()));
        assert_eq!(test.storage.count_ages_flushed(), 0);
        test.set_has_aged(0);
        assert!(!test.get_should_age(test.storage.current_age()));
        assert_eq!(test.storage.count_ages_flushed(), 1);
        // simulate rest of buckets aging
        for _ in 1..BINS_FOR_TESTING {
            assert!(!test.storage.all_buckets_flushed_at_current_age());
            test.storage.bucket_flushed_at_current_age();
        }
        assert!(test.storage.all_buckets_flushed_at_current_age());
        // advance age
        test.storage.increment_age();
        assert_eq!(test.storage.current_age(), 1);
        assert!(!test.storage.all_buckets_flushed_at_current_age());
        assert!(test.get_should_age(test.storage.current_age()));
        assert_eq!(test.storage.count_ages_flushed(), 0);
    }
}
