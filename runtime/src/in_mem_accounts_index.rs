use {
    crate::{
        accounts_index::{
            AccountMapEntry, AccountMapEntryInner, AccountMapEntryMeta, IndexValue,
            PreAllocatedAccountMapEntry, RefCount, SlotList, SlotSlice, UpsertReclaim, ZeroLamport,
        },
        bucket_map_holder::{Age, BucketMapHolder},
        bucket_map_holder_stats::BucketMapHolderStats,
        waitable_condvar::WaitableCondvar,
    },
    rand::{thread_rng, Rng},
    solana_bucket_map::bucket_api::BucketApi,
    solana_measure::measure::Measure,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{
        collections::{hash_map::Entry, HashMap},
        fmt::Debug,
        ops::{Bound, RangeBounds, RangeInclusive},
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
            Arc, Mutex, RwLock, RwLockWriteGuard,
        },
    },
};
type K = Pubkey;
type CacheRangesHeld = RwLock<Vec<RangeInclusive<Pubkey>>>;

type InMemMap<T> = HashMap<Pubkey, AccountMapEntry<T>>;

#[derive(Debug)]
pub struct PossibleEvictions<T: IndexValue> {
    /// vec per age in the future, up to size 'ages_to_stay_in_cache'
    possible_evictions: Vec<FlushScanResult<T>>,
    /// next index to use into 'possible_evictions'
    /// if 'index' >= 'possible_evictions.len()', then there are no available entries
    index: usize,
}

impl<T: IndexValue> PossibleEvictions<T> {
    fn new(max_ages: Age) -> Self {
        Self {
            possible_evictions: (0..max_ages).map(|_| FlushScanResult::default()).collect(),
            index: max_ages as usize, // initially no data
        }
    }

    /// remove the possible evictions. This is required because we need ownership of the Arc strong counts to transfer to caller so entries can be removed from the accounts index
    fn get_possible_evictions(&mut self) -> Option<FlushScanResult<T>> {
        self.possible_evictions.get_mut(self.index).map(|result| {
            self.index += 1;
            // remove the list from 'possible_evictions'
            std::mem::take(result)
        })
    }

    /// clear existing data and prepare to add 'entries' more ages of data
    fn reset(&mut self, entries: Age) {
        self.possible_evictions.iter_mut().for_each(|entry| {
            entry.evictions_random.clear();
            entry.evictions_age_possible.clear();
        });
        let entries = entries as usize;
        assert!(
            entries <= self.possible_evictions.len(),
            "entries: {}, len: {}",
            entries,
            self.possible_evictions.len()
        );
        self.index = self.possible_evictions.len() - entries;
    }

    /// insert 'entry' at 'relative_age' in the future into 'possible_evictions'
    fn insert(&mut self, relative_age: Age, key: Pubkey, entry: AccountMapEntry<T>, random: bool) {
        let index = self.index + (relative_age as usize);
        let list = &mut self.possible_evictions[index];
        if random {
            &mut list.evictions_random
        } else {
            &mut list.evictions_age_possible
        }
        .push((key, entry));
    }
}

// one instance of this represents one bin of the accounts index.
pub struct InMemAccountsIndex<T: IndexValue> {
    last_age_flushed: AtomicU8,

    // backing store
    map_internal: RwLock<InMemMap<T>>,
    storage: Arc<BucketMapHolder<T>>,
    bin: usize,

    bucket: Option<Arc<BucketApi<(Slot, T)>>>,

    // pubkey ranges that this bin must hold in the cache while the range is present in this vec
    pub(crate) cache_ranges_held: CacheRangesHeld,
    // incremented each time stop_evictions is changed
    stop_evictions_changes: AtomicU64,
    // true while ranges are being manipulated. Used to keep an async flush from removing things while a range is being held.
    stop_evictions: AtomicU64,
    // set to true while this bin is being actively flushed
    flushing_active: AtomicBool,

    /// info to streamline initial index generation
    startup_info: Mutex<StartupInfo<T>>,

    /// possible evictions for next few slots coming up
    possible_evictions: RwLock<PossibleEvictions<T>>,
    /// when age % ages_to_stay_in_cache == 'age_to_flush_bin_offset', then calculate the next 'ages_to_stay_in_cache' 'possible_evictions'
    /// this causes us to scan the entire in-mem hash map every 1/'ages_to_stay_in_cache' instead of each age
    age_to_flush_bin_mod: Age,
}

impl<T: IndexValue> Debug for InMemAccountsIndex<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

pub enum InsertNewEntryResults {
    DidNotExist,
    ExistedNewEntryZeroLamports,
    ExistedNewEntryNonZeroLamports,
}

#[derive(Default, Debug)]
struct StartupInfo<T: IndexValue> {
    /// entries to add next time we are flushing to disk
    insert: Vec<(Slot, Pubkey, T)>,
    /// pubkeys that were found to have duplicate index entries
    duplicates: Vec<(Slot, Pubkey)>,
}

#[derive(Default, Debug)]
/// result from scanning in-mem index during flush
struct FlushScanResult<T> {
    /// pubkeys whose age indicates they may be evicted now, pending further checks.
    evictions_age_possible: Vec<(Pubkey, AccountMapEntry<T>)>,
    /// pubkeys chosen to evict based on random eviction
    evictions_random: Vec<(Pubkey, AccountMapEntry<T>)>,
}

impl<T: IndexValue> InMemAccountsIndex<T> {
    pub fn new(storage: &Arc<BucketMapHolder<T>>, bin: usize) -> Self {
        let ages_to_stay_in_cache = storage.ages_to_stay_in_cache;
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
            stop_evictions_changes: AtomicU64::default(),
            stop_evictions: AtomicU64::default(),
            flushing_active: AtomicBool::default(),
            // initialize this to max, to make it clear we have not flushed at age 0, the starting age
            last_age_flushed: AtomicU8::new(Age::MAX),
            startup_info: Mutex::default(),
            possible_evictions: RwLock::new(PossibleEvictions::new(ages_to_stay_in_cache)),
            // Spread out the scanning across all ages within the window.
            // This causes us to scan 1/N of the bins each 'Age'
            age_to_flush_bin_mod: thread_rng().gen_range(0, ages_to_stay_in_cache),
        }
    }

    /// # ages to scan ahead
    fn ages_to_scan_ahead(&self, current_age: Age) -> Age {
        let ages_to_stay_in_cache = self.storage.ages_to_stay_in_cache;
        if (self.age_to_flush_bin_mod == current_age % ages_to_stay_in_cache)
            && !self.storage.get_startup()
        {
            // scan ahead multiple ages
            ages_to_stay_in_cache
        } else {
            1 // just current age
        }
    }

    /// true if this bucket needs to call flush for the current age
    /// we need to scan each bucket once per value of age
    fn get_should_age(&self, age: Age) -> bool {
        let last_age_flushed = self.last_age_flushed();
        last_age_flushed != age
    }

    /// called after flush scans this bucket at the current age
    fn set_has_aged(&self, age: Age, can_advance_age: bool) {
        self.last_age_flushed.store(age, Ordering::Release);
        self.storage.bucket_flushed_at_current_age(can_advance_age);
    }

    fn last_age_flushed(&self) -> Age {
        self.last_age_flushed.load(Ordering::Acquire)
    }

    /// Release entire in-mem hashmap to free all memory associated with it.
    /// Idea is that during startup we needed a larger map than we need during runtime.
    /// When using disk-buckets, in-mem index grows over time with dynamic use and then shrinks, in theory back to 0.
    pub fn shrink_to_fit(&self) {
        // shrink_to_fit could be quite expensive on large map sizes, which 'no disk buckets' could produce, so avoid shrinking in case we end up here
        if self.storage.is_disk_index_enabled() {
            self.map_internal.write().unwrap().shrink_to_fit();
        }
    }

    pub fn items<R>(&self, range: &R) -> Vec<(K, AccountMapEntry<T>)>
    where
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        let m = Measure::start("items");
        self.hold_range_in_memory(range, true);
        let map = self.map_internal.read().unwrap();
        let mut result = Vec::with_capacity(map.len());
        map.iter().for_each(|(k, v)| {
            if range.contains(k) {
                result.push((*k, Arc::clone(v)));
            }
        });
        drop(map);
        self.hold_range_in_memory(range, false);
        Self::update_stat(&self.stats().items, 1);
        Self::update_time_stat(&self.stats().items_us, m);
        result
    }

    // only called in debug code paths
    pub fn keys(&self) -> Vec<Pubkey> {
        Self::update_stat(&self.stats().keys, 1);
        // easiest implementation is to load evrything from disk into cache and return the keys
        let evictions_guard = EvictionsGuard::lock(self);
        self.put_range_in_cache(&None::<&RangeInclusive<Pubkey>>, &evictions_guard);
        let keys = self.map_internal.read().unwrap().keys().cloned().collect();
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
        update_age: bool,
        callback: impl for<'a> FnOnce(Option<&'a AccountMapEntry<T>>) -> RT,
    ) -> RT {
        let mut found = true;
        let mut m = Measure::start("get");
        let result = {
            let map = self.map_internal.read().unwrap();
            let result = map.get(pubkey);
            m.stop();

            callback(if let Some(entry) = result {
                if update_age {
                    self.set_age_to_future(entry, false);
                }
                Some(entry)
            } else {
                drop(map);
                found = false;
                None
            })
        };

        let stats = self.stats();
        let (count, time) = if found {
            (&stats.gets_from_mem, &stats.get_mem_us)
        } else {
            (&stats.gets_missing, &stats.get_missing_us)
        };
        Self::update_stat(time, m.as_us());
        Self::update_stat(count, 1);

        result
    }

    /// lookup 'pubkey' in index (in mem or on disk)
    pub fn get(&self, pubkey: &K) -> Option<AccountMapEntry<T>> {
        self.get_internal(pubkey, |entry| (true, entry.map(Arc::clone)))
    }

    /// set age of 'entry' to the future
    /// if 'is_cached', age will be set farther
    fn set_age_to_future(&self, entry: &AccountMapEntry<T>, is_cached: bool) {
        entry.set_age(self.storage.future_age_to_flush(is_cached));
    }

    /// lookup 'pubkey' in index (in_mem or disk).
    /// call 'callback' whether found or not
    pub(crate) fn get_internal<RT>(
        &self,
        pubkey: &K,
        // return true if item should be added to in_mem cache
        callback: impl for<'a> FnOnce(Option<&AccountMapEntry<T>>) -> (bool, RT),
    ) -> RT {
        self.get_only_in_mem(pubkey, true, |entry| {
            if let Some(entry) = entry {
                callback(Some(entry)).1
            } else {
                // not in cache, look on disk
                let stats = self.stats();
                let disk_entry = self.load_account_entry_from_disk(pubkey);
                if disk_entry.is_none() {
                    return callback(None).1;
                }
                let disk_entry = disk_entry.unwrap();
                let mut map = self.map_internal.write().unwrap();
                let entry = map.entry(*pubkey);
                match entry {
                    Entry::Occupied(occupied) => callback(Some(occupied.get())).1,
                    Entry::Vacant(vacant) => {
                        let (add_to_cache, rt) = callback(Some(&disk_entry));

                        if add_to_cache {
                            stats.inc_mem_count(self.bin);
                            vacant.insert(disk_entry);
                        }
                        rt
                    }
                }
            }
        })
    }

    fn remove_if_slot_list_empty_value(&self, slot_list: SlotSlice<T>) -> bool {
        if slot_list.is_empty() {
            self.stats().inc_delete();
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

    /// return false if the entry is in the index (disk or memory) and has a slot list len > 0
    /// return true in all other cases, including if the entry is NOT in the index at all
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
                    //  the arc, but someone may already have retrieved a clone of it.
                    // account index in_mem flushing is one such possibility
                    self.delete_disk_key(occupied.key());
                    self.stats().dec_mem_count(self.bin);
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
                    None => true, // not in cache or on disk, but slot list is 'empty' and entry is not in index, so return true
                }
            }
        }
    }

    // If the slot list for pubkey exists in the index and is empty, remove the index entry for pubkey and return true.
    // Return false otherwise.
    pub fn remove_if_slot_list_empty(&self, pubkey: Pubkey) -> bool {
        let mut m = Measure::start("entry");
        let mut map = self.map_internal.write().unwrap();
        let entry = map.entry(pubkey);
        m.stop();
        let found = matches!(entry, Entry::Occupied(_));
        let result = self.remove_if_slot_list_empty_entry(entry);
        drop(map);

        self.update_entry_stats(m, found);
        result
    }

    pub fn slot_list_mut<RT>(
        &self,
        pubkey: &Pubkey,
        user: impl for<'a> FnOnce(&mut RwLockWriteGuard<'a, SlotList<T>>) -> RT,
    ) -> Option<RT> {
        self.get_internal(pubkey, |entry| {
            (
                true,
                entry.map(|entry| {
                    let result = user(&mut entry.slot_list.write().unwrap());
                    entry.set_dirty(true);
                    result
                }),
            )
        })
    }

    /// update 'entry' with 'new_value'
    fn update_slot_list_entry(
        &self,
        entry: &AccountMapEntry<T>,
        new_value: PreAllocatedAccountMapEntry<T>,
        other_slot: Option<Slot>,
        reclaims: &mut SlotList<T>,
        reclaim: UpsertReclaim,
    ) {
        let new_value: (Slot, T) = new_value.into();
        let mut upsert_cached = new_value.1.is_cached();
        if Self::lock_and_update_slot_list(entry, new_value, other_slot, reclaims, reclaim) > 1 {
            // if slot list > 1, then we are going to hold this entry in memory until it gets set back to 1
            upsert_cached = true;
        }
        self.set_age_to_future(entry, upsert_cached);
    }

    pub fn upsert(
        &self,
        pubkey: &Pubkey,
        new_value: PreAllocatedAccountMapEntry<T>,
        other_slot: Option<Slot>,
        reclaims: &mut SlotList<T>,
        reclaim: UpsertReclaim,
    ) {
        let mut updated_in_mem = true;
        // try to get it just from memory first using only a read lock
        self.get_only_in_mem(pubkey, false, |entry| {
            if let Some(entry) = entry {
                self.update_slot_list_entry(entry, new_value, other_slot, reclaims, reclaim);
            } else {
                let mut m = Measure::start("entry");
                let mut map = self.map_internal.write().unwrap();
                let entry = map.entry(*pubkey);
                m.stop();
                let found = matches!(entry, Entry::Occupied(_));
                match entry {
                    Entry::Occupied(mut occupied) => {
                        let current = occupied.get_mut();
                        self.update_slot_list_entry(
                            current, new_value, other_slot, reclaims, reclaim,
                        );
                    }
                    Entry::Vacant(vacant) => {
                        // not in cache, look on disk
                        updated_in_mem = false;

                        // go to in-mem cache first
                        let disk_entry = self.load_account_entry_from_disk(vacant.key());
                        let new_value = if let Some(disk_entry) = disk_entry {
                            // on disk, so merge new_value with what was on disk
                            self.update_slot_list_entry(
                                &disk_entry,
                                new_value,
                                other_slot,
                                reclaims,
                                reclaim,
                            );
                            disk_entry
                        } else {
                            // not on disk, so insert new thing
                            self.stats().inc_insert();
                            new_value.into_account_map_entry(&self.storage)
                        };
                        assert!(new_value.dirty());
                        vacant.insert(new_value);
                        self.stats().inc_mem_count(self.bin);
                    }
                };

                drop(map);
                self.update_entry_stats(m, found);
            };
        });
        if updated_in_mem {
            Self::update_stat(&self.stats().updates_in_mem, 1);
        }
    }

    fn update_entry_stats(&self, stopped_measure: Measure, found: bool) {
        let stats = self.stats();
        let (count, time) = if found {
            (&stats.entries_from_mem, &stats.entry_mem_us)
        } else {
            (&stats.entries_missing, &stats.entry_missing_us)
        };
        Self::update_stat(time, stopped_measure.as_us());
        Self::update_stat(count, 1);
    }

    /// Try to update an item in the slot list the given `slot` If an item for the slot
    /// already exists in the list, remove the older item, add it to `reclaims`, and insert
    /// the new item.
    /// if 'other_slot' is some, then also remove any entries in the slot list that are at 'other_slot'
    /// return resulting len of slot list
    pub(crate) fn lock_and_update_slot_list(
        current: &AccountMapEntryInner<T>,
        new_value: (Slot, T),
        other_slot: Option<Slot>,
        reclaims: &mut SlotList<T>,
        reclaim: UpsertReclaim,
    ) -> usize {
        let mut slot_list = current.slot_list.write().unwrap();
        let (slot, new_entry) = new_value;
        let addref = Self::update_slot_list(
            &mut slot_list,
            slot,
            new_entry,
            other_slot,
            reclaims,
            reclaim,
        );
        if addref {
            current.addref();
        }
        current.set_dirty(true);
        slot_list.len()
    }

    /// modifies slot_list
    /// any entry at 'slot' or slot 'other_slot' is replaced with 'account_info'.
    /// or, 'account_info' is appended to the slot list if the slot did not exist previously.
    /// returns true if caller should addref
    /// conditions when caller should addref:
    ///   'account_info' does NOT represent a cached storage (the slot is being flushed from the cache)
    /// AND
    ///   previous slot_list entry AT 'slot' did not exist (this is the first time this account was modified in this "slot"), or was previously cached (the storage is now being flushed from the cache)
    /// Note that even if entry DID exist at 'other_slot', the above conditions apply.
    fn update_slot_list(
        slot_list: &mut SlotList<T>,
        slot: Slot,
        account_info: T,
        mut other_slot: Option<Slot>,
        reclaims: &mut SlotList<T>,
        reclaim: UpsertReclaim,
    ) -> bool {
        let mut addref = !account_info.is_cached();

        if other_slot == Some(slot) {
            other_slot = None; // redundant info, so ignore
        }

        // There may be 0..=2 dirty accounts found (one at 'slot' and one at 'other_slot')
        // that are already in the slot list.  Since the first one found will be swapped with the
        // new account, if a second one is found, we cannot swap again. Instead, just remove it.
        let mut found_slot = false;
        let mut found_other_slot = false;
        (0..slot_list.len())
            .rev() // rev since we delete from the list in some cases
            .for_each(|slot_list_index| {
                let (cur_slot, cur_account_info) = &slot_list[slot_list_index];
                let matched_slot = *cur_slot == slot;
                if matched_slot || Some(*cur_slot) == other_slot {
                    // make sure neither 'slot' nor 'other_slot' are in the slot list more than once
                    let matched_other_slot = !matched_slot;
                    assert!(
                        !(found_slot && matched_slot || matched_other_slot && found_other_slot),
                        "{slot_list:?}, slot: {slot}, other_slot: {other_slot:?}"
                    );

                    let is_cur_account_cached = cur_account_info.is_cached();

                    let reclaim_item = if !(found_slot || found_other_slot) {
                        // first time we found an entry in 'slot' or 'other_slot', so replace it in-place.
                        // this may be the only instance we find
                        std::mem::replace(&mut slot_list[slot_list_index], (slot, account_info))
                    } else {
                        // already replaced one entry, so this one has to be removed
                        slot_list.remove(slot_list_index)
                    };
                    match reclaim {
                        UpsertReclaim::PopulateReclaims => {
                            reclaims.push(reclaim_item);
                        }
                        UpsertReclaim::PreviousSlotEntryWasCached => {
                            assert!(is_cur_account_cached);
                        }
                        UpsertReclaim::IgnoreReclaims => {
                            // do nothing. nothing to assert. nothing to return in reclaims
                        }
                    }

                    if matched_slot {
                        found_slot = true;
                    } else {
                        found_other_slot = true;
                    }
                    if !is_cur_account_cached {
                        // current info at 'slot' is NOT cached, so we should NOT addref. This slot already has a ref count for this pubkey.
                        addref = false;
                    }
                }
            });
        if !found_slot && !found_other_slot {
            // if we make it here, we did not find the slot in the list
            slot_list.push((slot, account_info));
        }
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
            AccountMapEntryMeta::new_clean(&self.storage),
        ))
    }

    pub fn len_for_stats(&self) -> usize {
        self.stats().count_in_bucket(self.bin)
    }

    /// Queue up these insertions for when the flush thread is dealing with this bin.
    /// This is very fast and requires no lookups or disk access.
    pub fn startup_insert_only(&self, slot: Slot, items: impl Iterator<Item = (Pubkey, T)>) {
        assert!(self.storage.get_startup());
        assert!(self.bucket.is_some());

        let insert = &mut self.startup_info.lock().unwrap().insert;
        items
            .into_iter()
            .for_each(|(k, v)| insert.push((slot, k, v)));
    }

    pub fn insert_new_entry_if_missing_with_lock(
        &self,
        pubkey: Pubkey,
        new_entry: PreAllocatedAccountMapEntry<T>,
    ) -> InsertNewEntryResults {
        let mut m = Measure::start("entry");
        let mut map = self.map_internal.write().unwrap();
        let entry = map.entry(pubkey);
        m.stop();
        let new_entry_zero_lamports = new_entry.is_zero_lamport();
        let (found_in_mem, already_existed) = match entry {
            Entry::Occupied(occupied) => {
                // in cache, so merge into cache
                let (slot, account_info) = new_entry.into();
                InMemAccountsIndex::lock_and_update_slot_list(
                    occupied.get(),
                    (slot, account_info),
                    None, // should be None because we don't expect a different slot # during index generation
                    &mut Vec::default(),
                    UpsertReclaim::PopulateReclaims, // this should be ignore?
                );
                (
                    true, /* found in mem */
                    true, /* already existed */
                )
            }
            Entry::Vacant(vacant) => {
                // not in cache, look on disk
                let disk_entry = self.load_account_entry_from_disk(vacant.key());
                self.stats().inc_mem_count(self.bin);
                if let Some(disk_entry) = disk_entry {
                    let (slot, account_info) = new_entry.into();
                    InMemAccountsIndex::lock_and_update_slot_list(
                        &disk_entry,
                        (slot, account_info),
                        // None because we are inserting the first element in the slot list for this pubkey.
                        // There can be no 'other' slot in the list.
                        None,
                        &mut Vec::default(),
                        UpsertReclaim::PopulateReclaims,
                    );
                    vacant.insert(disk_entry);
                    (
                        false, /* found in mem */
                        true,  /* already existed */
                    )
                } else {
                    // not on disk, so insert new thing and we're done
                    let new_entry: AccountMapEntry<T> =
                        new_entry.into_account_map_entry(&self.storage);
                    assert!(new_entry.dirty());
                    vacant.insert(new_entry);
                    (false, false)
                }
            }
        };
        drop(map);
        self.update_entry_stats(m, found_in_mem);
        let stats = self.stats();
        if !already_existed {
            stats.inc_insert();
        } else {
            Self::update_stat(&stats.updates_in_mem, 1);
        }
        if !already_existed {
            InsertNewEntryResults::DidNotExist
        } else if new_entry_zero_lamports {
            InsertNewEntryResults::ExistedNewEntryZeroLamports
        } else {
            InsertNewEntryResults::ExistedNewEntryNonZeroLamports
        }
    }

    /// Look at the currently held ranges. If 'range' is already included in what is
    ///  being held, then add 'range' to the currently held list AND return true
    /// If 'range' is NOT already included in what is being held, then return false
    ///  withOUT adding 'range' to the list of what is currently held
    fn add_hold_range_in_memory_if_already_held<R>(
        &self,
        range: &R,
        evictions_guard: &EvictionsGuard,
    ) -> bool
    where
        R: RangeBounds<Pubkey>,
    {
        let start_holding = true;
        let only_add_if_already_held = true;
        self.just_set_hold_range_in_memory_internal(
            range,
            start_holding,
            only_add_if_already_held,
            evictions_guard,
        )
    }

    fn just_set_hold_range_in_memory<R>(
        &self,
        range: &R,
        start_holding: bool,
        evictions_guard: &EvictionsGuard,
    ) where
        R: RangeBounds<Pubkey>,
    {
        let only_add_if_already_held = false;
        let _ = self.just_set_hold_range_in_memory_internal(
            range,
            start_holding,
            only_add_if_already_held,
            evictions_guard,
        );
    }

    /// if 'start_holding', then caller wants to add 'range' to the list of ranges being held
    /// if !'start_holding', then caller wants to remove 'range' to the list
    /// if 'only_add_if_already_held', caller intends to only add 'range' to the list if the range is already held
    /// returns true iff start_holding=true and the range we're asked to hold was already being held
    fn just_set_hold_range_in_memory_internal<R>(
        &self,
        range: &R,
        start_holding: bool,
        only_add_if_already_held: bool,
        _evictions_guard: &EvictionsGuard,
    ) -> bool
    where
        R: RangeBounds<Pubkey>,
    {
        assert!(!only_add_if_already_held || start_holding);
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
        let inclusive_range = start..=end;
        let mut ranges = self.cache_ranges_held.write().unwrap();
        let mut already_held = false;
        if start_holding {
            if only_add_if_already_held {
                for r in ranges.iter() {
                    if r.contains(&start) && r.contains(&end) {
                        already_held = true;
                        break;
                    }
                }
            }
            if already_held || !only_add_if_already_held {
                ranges.push(inclusive_range);
            }
        } else {
            // find the matching range and delete it since we don't want to hold it anymore
            // search backwards, assuming LIFO ordering
            for (i, r) in ranges.iter().enumerate().rev() {
                if let (Bound::Included(start_found), Bound::Included(end_found)) =
                    (r.start_bound(), r.end_bound())
                {
                    if start_found == &start && end_found == &end {
                        // found a match. There may be dups, that's ok, we expect another call to remove the dup.
                        ranges.remove(i);
                        break;
                    }
                }
            }
        }
        already_held
    }

    /// if 'start_holding'=true, then:
    ///  at the end of this function, cache_ranges_held will be updated to contain 'range'
    ///  and all pubkeys in that range will be in the in-mem cache
    /// if 'start_holding'=false, then:
    ///  'range' will be removed from cache_ranges_held
    ///  and all pubkeys will be eligible for being removed from in-mem cache in the bg if no other range is holding them
    /// Any in-process flush will be aborted when it gets to evicting items from in-mem.
    pub fn hold_range_in_memory<R>(&self, range: &R, start_holding: bool)
    where
        R: RangeBounds<Pubkey> + Debug,
    {
        let evictions_guard = EvictionsGuard::lock(self);

        if !start_holding || !self.add_hold_range_in_memory_if_already_held(range, &evictions_guard)
        {
            if start_holding {
                // put everything in the cache and it will be held there
                self.put_range_in_cache(&Some(range), &evictions_guard);
            }
            // do this AFTER items have been put in cache - that way anyone who finds this range can know that the items are already in the cache
            self.just_set_hold_range_in_memory(range, start_holding, &evictions_guard);
        }
    }

    fn put_range_in_cache<R>(&self, range: &Option<&R>, _evictions_guard: &EvictionsGuard)
    where
        R: RangeBounds<Pubkey>,
    {
        assert!(self.get_stop_evictions()); // caller should be controlling the lifetime of how long this needs to be present
        let m = Measure::start("range");

        let mut added_to_mem = 0;
        // load from disk
        if let Some(disk) = self.bucket.as_ref() {
            let mut map = self.map_internal.write().unwrap();
            let items = disk.items_in_range(range); // map's lock has to be held while we are getting items from disk
            let future_age = self.storage.future_age_to_flush(false);
            for item in items {
                let entry = map.entry(item.pubkey);
                match entry {
                    Entry::Occupied(occupied) => {
                        // item already in cache, bump age to future. This helps the current age flush to succeed.
                        occupied.get().set_age(future_age);
                    }
                    Entry::Vacant(vacant) => {
                        vacant.insert(self.disk_to_cache_entry(item.slot_list, item.ref_count));
                        added_to_mem += 1;
                    }
                }
            }
        }
        self.stats().add_mem_count(self.bin, added_to_mem);

        Self::update_time_stat(&self.stats().get_range_us, m);
    }

    /// returns true if there are active requests to stop evictions
    fn get_stop_evictions(&self) -> bool {
        self.stop_evictions.load(Ordering::Acquire) > 0
    }

    /// return count of calls to 'start_stop_evictions', indicating changes could have been made to eviction strategy
    fn get_stop_evictions_changes(&self) -> u64 {
        self.stop_evictions_changes.load(Ordering::Acquire)
    }

    pub(crate) fn flush(&self, can_advance_age: bool) {
        if let Some(flush_guard) = FlushGuard::lock(&self.flushing_active) {
            self.flush_internal(&flush_guard, can_advance_age)
        }
    }

    /// returns true if a dice roll indicates this call should result in a random eviction.
    /// This causes non-determinism in cache contents per validator.
    fn random_chance_of_eviction() -> bool {
        // random eviction
        const N: usize = 1000;
        // 1/N chance of eviction
        thread_rng().gen_range(0, N) == 0
    }

    /// assumes 1 entry in the slot list. Ignores overhead of the HashMap and such
    fn approx_size_of_one_entry() -> usize {
        std::mem::size_of::<T>()
            + std::mem::size_of::<Pubkey>()
            + std::mem::size_of::<AccountMapEntry<T>>()
    }

    fn should_evict_based_on_age(
        current_age: Age,
        entry: &AccountMapEntry<T>,
        startup: bool,
    ) -> bool {
        startup || (current_age == entry.age())
    }

    /// return true if 'entry' should be evicted from the in-mem index
    fn should_evict_from_mem<'a>(
        &self,
        current_age: Age,
        entry: &'a AccountMapEntry<T>,
        startup: bool,
        update_stats: bool,
        exceeds_budget: bool,
    ) -> (bool, Option<std::sync::RwLockReadGuard<'a, SlotList<T>>>) {
        // this could be tunable dynamically based on memory pressure
        // we could look at more ages or we could throw out more items we are choosing to keep in the cache
        if Self::should_evict_based_on_age(current_age, entry, startup) {
            if exceeds_budget {
                // if we are already holding too many items in-mem, then we need to be more aggressive at kicking things out
                (true, None)
            } else {
                // only read the slot list if we are planning to throw the item out
                let slot_list = entry.slot_list.read().unwrap();
                if slot_list.len() != 1 {
                    if update_stats {
                        Self::update_stat(&self.stats().held_in_mem_slot_list_len, 1);
                    }
                    (false, None) // keep 0 and > 1 slot lists in mem. They will be cleaned or shrunk soon.
                } else {
                    // keep items with slot lists that contained cached items
                    let evict = !slot_list.iter().any(|(_, info)| info.is_cached());
                    if !evict && update_stats {
                        Self::update_stat(&self.stats().held_in_mem_slot_list_cached, 1);
                    }
                    (evict, if evict { Some(slot_list) } else { None })
                }
            }
        } else {
            (false, None)
        }
    }

    /// scan loop
    /// holds read lock
    /// identifies items which are dirty and items to evict
    fn flush_scan(
        &self,
        current_age: Age,
        startup: bool,
        _flush_guard: &FlushGuard,
    ) -> FlushScanResult<T> {
        let mut possible_evictions = self.possible_evictions.write().unwrap();
        if let Some(result) = possible_evictions.get_possible_evictions() {
            // we have previously calculated the possible evictions for this age
            return result;
        }
        // otherwise, we need to scan some number of ages into the future now
        let ages_to_scan = self.ages_to_scan_ahead(current_age);
        possible_evictions.reset(ages_to_scan);

        let m;
        {
            let map = self.map_internal.read().unwrap();
            m = Measure::start("flush_scan"); // we don't care about lock time in this metric - bg threads can wait
            for (k, v) in map.iter() {
                let random = Self::random_chance_of_eviction();
                let age_offset = if random {
                    thread_rng().gen_range(0, ages_to_scan)
                } else if startup {
                    0
                } else {
                    let ages_in_future = v.age().wrapping_sub(current_age);
                    if ages_in_future >= ages_to_scan {
                        // not planning to evict this item from memory within the next few ages
                        continue;
                    }
                    ages_in_future
                };

                possible_evictions.insert(age_offset, *k, Arc::clone(v), random);
            }
        }
        Self::update_time_stat(&self.stats().flush_scan_us, m);

        possible_evictions.get_possible_evictions().unwrap()
    }

    fn write_startup_info_to_disk(&self) {
        let insert = std::mem::take(&mut self.startup_info.lock().unwrap().insert);
        if insert.is_empty() {
            // nothing to insert for this bin
            return;
        }

        // during startup, nothing should be in the in-mem map
        let map_internal = self.map_internal.read().unwrap();
        assert!(
            map_internal.is_empty(),
            "len: {}, first: {:?}",
            map_internal.len(),
            map_internal.iter().take(1).collect::<Vec<_>>()
        );
        drop(map_internal);

        let mut duplicates = vec![];

        // merge all items into the disk index now
        let disk = self.bucket.as_ref().unwrap();
        let mut count = 0;
        insert.into_iter().for_each(|(slot, k, v)| {
            let entry = (slot, v);
            let new_ref_count = u64::from(!v.is_cached());
            disk.update(&k, |current| {
                match current {
                    Some((current_slot_list, mut ref_count)) => {
                        // merge this in, mark as conflict
                        let mut slot_list = Vec::with_capacity(current_slot_list.len() + 1);
                        slot_list.extend_from_slice(current_slot_list);
                        slot_list.push(entry); // will never be from the same slot that already exists in the list
                        ref_count += new_ref_count;
                        duplicates.push((slot, k));
                        Some((slot_list, ref_count))
                    }
                    None => {
                        count += 1;
                        // not on disk, insert it
                        Some((vec![entry], new_ref_count))
                    }
                }
            });
        });
        self.stats().inc_insert_count(count);
        self.startup_info
            .lock()
            .unwrap()
            .duplicates
            .append(&mut duplicates);
    }

    /// pull out all duplicate pubkeys from 'startup_info'
    /// duplicate pubkeys have a slot list with len > 1
    /// These were collected for this bin when we did batch inserts in the bg flush threads.
    pub fn retrieve_duplicate_keys_from_startup(&self) -> Vec<(Slot, Pubkey)> {
        let mut write = self.startup_info.lock().unwrap();
        // in order to return accurate and complete duplicates, we must have nothing left remaining to insert
        assert!(write.insert.is_empty());

        std::mem::take(&mut write.duplicates)
    }

    /// synchronize the in-mem index with the disk index
    fn flush_internal(&self, flush_guard: &FlushGuard, can_advance_age: bool) {
        let current_age = self.storage.current_age();
        let iterate_for_age = self.get_should_age(current_age);
        let startup = self.storage.get_startup();
        if !iterate_for_age && !startup {
            // no need to age, so no need to flush this bucket
            // but, at startup we want to evict from buckets as fast as possible if any items exist
            return;
        }

        // scan in-mem map for items that we may evict
        let FlushScanResult {
            mut evictions_age_possible,
            mut evictions_random,
        } = self.flush_scan(current_age, startup, flush_guard);

        if startup {
            self.write_startup_info_to_disk();
        }

        // write to disk outside in-mem map read lock
        {
            let mut evictions_age = Vec::with_capacity(evictions_age_possible.len());
            if !evictions_age_possible.is_empty() || !evictions_random.is_empty() {
                let disk = self.bucket.as_ref().unwrap();
                let mut flush_entries_updated_on_disk = 0;
                let exceeds_budget = self.get_exceeds_budget();
                let mut flush_should_evict_us = 0;
                // we don't care about lock time in this metric - bg threads can wait
                let m = Measure::start("flush_update");

                // consider whether to write to disk for all the items we may evict, whether evicting due to age or random
                for (is_random, check_for_eviction_and_dirty) in [
                    (false, &mut evictions_age_possible),
                    (true, &mut evictions_random),
                ] {
                    for (k, v) in check_for_eviction_and_dirty.drain(..) {
                        let mut slot_list = None;
                        if !is_random {
                            let mut mse = Measure::start("flush_should_evict");
                            let (evict_for_age, slot_list_temp) = self.should_evict_from_mem(
                                current_age,
                                &v,
                                startup,
                                true,
                                exceeds_budget,
                            );
                            slot_list = slot_list_temp;
                            mse.stop();
                            flush_should_evict_us += mse.as_us();
                            if evict_for_age {
                                evictions_age.push(k);
                            } else {
                                // not evicting, so don't write, even if dirty
                                continue;
                            }
                        }
                        // if we are evicting it, then we need to update disk if we're dirty
                        if v.clear_dirty() {
                            // step 1: clear the dirty flag
                            // step 2: perform the update on disk based on the fields in the entry
                            // If a parallel operation dirties the item again - even while this flush is occurring,
                            //  the last thing the writer will do, after updating contents, is set_dirty(true)
                            //  That prevents dropping an item from cache before disk is updated to latest in mem.
                            // It is possible that the item in the cache is marked as dirty while these updates are happening. That is ok.
                            //  The dirty will be picked up and the item will be prevented from being evicted.

                            // may have to loop if disk has to grow and we have to retry the write
                            loop {
                                let disk_resize = {
                                    let slot_list = slot_list
                                        .take()
                                        .unwrap_or_else(|| v.slot_list.read().unwrap());
                                    disk.try_write(&k, (&slot_list, v.ref_count()))
                                };
                                match disk_resize {
                                    Ok(_) => {
                                        // successfully written to disk
                                        flush_entries_updated_on_disk += 1;
                                        break;
                                    }
                                    Err(err) => {
                                        // disk needs to resize. This item did not get written. Resize and try again.
                                        let m = Measure::start("flush_grow");
                                        disk.grow(err);
                                        Self::update_time_stat(&self.stats().flush_grow_us, m);
                                    }
                                }
                            }
                        }
                    }
                }
                Self::update_time_stat(&self.stats().flush_update_us, m);
                Self::update_stat(&self.stats().flush_should_evict_us, flush_should_evict_us);
                Self::update_stat(
                    &self.stats().flush_entries_updated_on_disk,
                    flush_entries_updated_on_disk,
                );
                // remove the 'v'
                let evictions_random = evictions_random
                    .into_iter()
                    .map(|(k, _v)| k)
                    .collect::<Vec<_>>();

                let m = Measure::start("flush_evict");
                self.evict_from_cache(evictions_age, current_age, startup, false);
                self.evict_from_cache(evictions_random, current_age, startup, true);
                Self::update_time_stat(&self.stats().flush_evict_us, m);
            }

            if iterate_for_age {
                // completed iteration of the buckets at the current age
                assert_eq!(current_age, self.storage.current_age());
                self.set_has_aged(current_age, can_advance_age);
            }
        }
    }

    /// calculate the estimated size of the in-mem index
    /// return whether the size exceeds the specified budget
    fn get_exceeds_budget(&self) -> bool {
        let in_mem_count = self.stats().count_in_mem.load(Ordering::Relaxed);
        let limit = self.storage.mem_budget_mb;
        let estimate_mem = in_mem_count * Self::approx_size_of_one_entry();
        let exceeds_budget = limit
            .map(|limit| estimate_mem >= limit * 1024 * 1024)
            .unwrap_or_default();
        self.stats()
            .estimate_mem
            .store(estimate_mem as u64, Ordering::Relaxed);
        exceeds_budget
    }

    /// for each key in 'keys', look up in map, set age to the future
    fn move_ages_to_future(&self, next_age: Age, current_age: Age, keys: &[Pubkey]) {
        let map = self.map_internal.read().unwrap();
        keys.iter().for_each(|key| {
            if let Some(entry) = map.get(key) {
                entry.try_exchange_age(next_age, current_age);
            }
        });
    }

    // evict keys in 'evictions' from in-mem cache, likely due to age
    fn evict_from_cache(
        &self,
        mut evictions: Vec<Pubkey>,
        current_age: Age,
        startup: bool,
        randomly_evicted: bool,
    ) {
        if evictions.is_empty() {
            return;
        }

        let stop_evictions_changes_at_start = self.get_stop_evictions_changes();
        let next_age_on_failure = self.storage.future_age_to_flush(false);
        if self.get_stop_evictions() {
            // ranges were changed
            self.move_ages_to_future(next_age_on_failure, current_age, &evictions);
            return;
        }

        let mut failed = 0;

        // skip any keys that are held in memory because of ranges being held
        let ranges = self.cache_ranges_held.read().unwrap().clone();
        if !ranges.is_empty() {
            let mut move_age = Vec::default();
            evictions.retain(|k| {
                if ranges.iter().any(|range| range.contains(k)) {
                    // this item is held in mem by range, so don't evict
                    move_age.push(*k);
                    false
                } else {
                    true
                }
            });
            if !move_age.is_empty() {
                failed += move_age.len();
                self.move_ages_to_future(next_age_on_failure, current_age, &move_age);
            }
        }

        let mut evicted = 0;
        // chunk these so we don't hold the write lock too long
        for evictions in evictions.chunks(50) {
            let mut map = self.map_internal.write().unwrap();
            for k in evictions {
                if let Entry::Occupied(occupied) = map.entry(*k) {
                    let v = occupied.get();
                    if Arc::strong_count(v) > 1 {
                        // someone is holding the value arc's ref count and could modify it, so do not evict
                        failed += 1;
                        v.try_exchange_age(next_age_on_failure, current_age);
                        continue;
                    }

                    if v.dirty()
                        || (!randomly_evicted
                            && !Self::should_evict_based_on_age(current_age, v, startup))
                    {
                        // marked dirty or bumped in age after we looked above
                        // these evictions will be handled in later passes (at later ages)
                        // but, at startup, everything is ready to age out if it isn't dirty
                        failed += 1;
                        continue;
                    }

                    if stop_evictions_changes_at_start != self.get_stop_evictions_changes() {
                        // ranges were changed
                        failed += 1;
                        v.try_exchange_age(next_age_on_failure, current_age);
                        continue;
                    }

                    // all conditions for eviction succeeded, so really evict item from in-mem cache
                    evicted += 1;
                    occupied.remove();
                }
            }
            if map.is_empty() {
                map.shrink_to_fit();
            }
        }
        self.stats().sub_mem_count(self.bin, evicted);
        Self::update_stat(&self.stats().flush_entries_evicted_from_mem, evicted as u64);
        Self::update_stat(&self.stats().failed_to_evict, failed as u64);
    }

    pub fn stats(&self) -> &BucketMapHolderStats {
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

/// An RAII implementation of a scoped lock for the `flushing_active` atomic flag in
/// `InMemAccountsIndex`.  When this structure is dropped (falls out of scope), the flag will be
/// cleared (set to false).
///
/// After successfully locking (calling `FlushGuard::lock()`), pass a reference to the `FlashGuard`
/// instance to any function/code that requires the `flushing_active` flag has been set (to true).
#[derive(Debug)]
struct FlushGuard<'a> {
    flushing: &'a AtomicBool,
}

impl<'a> FlushGuard<'a> {
    /// Set the `flushing` atomic flag to true.  If the flag was already true, then return `None`
    /// (so as to not clear the flag erroneously).  Otherwise return `Some(FlushGuard)`.
    #[must_use = "if unused, the `flushing` flag will immediately clear"]
    fn lock(flushing: &'a AtomicBool) -> Option<Self> {
        let already_flushing = flushing.swap(true, Ordering::AcqRel);
        // Eager evaluation here would result in dropping Self and clearing flushing flag
        #[allow(clippy::unnecessary_lazy_evaluations)]
        (!already_flushing).then(|| Self { flushing })
    }
}

impl Drop for FlushGuard<'_> {
    fn drop(&mut self) {
        self.flushing.store(false, Ordering::Release);
    }
}

/// Disable (and safely enable) the background flusher from evicting entries from the in-mem
/// accounts index.  When disabled, no entries may be evicted.  When enabled, only eligible entries
/// may be evicted (i.e. those not in a held range).
///
/// An RAII implementation of a scoped lock for the `stop_evictions` atomic flag/counter in
/// `InMemAccountsIndex`.  When this structure is dropped (falls out of scope), the counter will
/// decrement and conditionally notify its storage.
///
/// After successfully locking (calling `EvictionsGuard::lock()`), pass a reference to the
/// `EvictionsGuard` instance to any function/code that requires `stop_evictions` to be
/// incremented/decremented correctly.
#[derive(Debug)]
struct EvictionsGuard<'a> {
    /// The number of active callers disabling evictions
    stop_evictions: &'a AtomicU64,
    /// The number of times that evictions have been disabled or enabled
    num_state_changes: &'a AtomicU64,
    /// Who will be notified after the evictions are re-enabled
    storage_notifier: &'a WaitableCondvar,
}

impl<'a> EvictionsGuard<'a> {
    #[must_use = "if unused, this evictions lock will be immediately unlocked"]
    fn lock<T: IndexValue>(in_mem_accounts_index: &'a InMemAccountsIndex<T>) -> Self {
        Self::lock_with(
            &in_mem_accounts_index.stop_evictions,
            &in_mem_accounts_index.stop_evictions_changes,
            &in_mem_accounts_index.storage.wait_dirty_or_aged,
        )
    }

    #[must_use = "if unused, this evictions lock will be immediately unlocked"]
    fn lock_with(
        stop_evictions: &'a AtomicU64,
        num_state_changes: &'a AtomicU64,
        storage_notifier: &'a WaitableCondvar,
    ) -> Self {
        num_state_changes.fetch_add(1, Ordering::Release);
        stop_evictions.fetch_add(1, Ordering::Release);

        Self {
            stop_evictions,
            num_state_changes,
            storage_notifier,
        }
    }
}

impl Drop for EvictionsGuard<'_> {
    fn drop(&mut self) {
        let previous_value = self.stop_evictions.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous_value > 0);

        let should_notify = previous_value == 1;
        if should_notify {
            // stop_evictions went to 0, so this bucket could now be ready to be aged
            self.storage_notifier.notify_one();
        }

        self.num_state_changes.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::accounts_index::{AccountsIndexConfig, IndexLimitMb, BINS_FOR_TESTING},
        itertools::Itertools,
    };

    fn new_for_test<T: IndexValue>() -> InMemAccountsIndex<T> {
        let holder = Arc::new(BucketMapHolder::new(
            BINS_FOR_TESTING,
            &Some(AccountsIndexConfig::default()),
            1,
        ));
        let bin = 0;
        InMemAccountsIndex::new(&holder, bin)
    }

    fn new_disk_buckets_for_test<T: IndexValue>() -> InMemAccountsIndex<T> {
        let holder = Arc::new(BucketMapHolder::new(
            BINS_FOR_TESTING,
            &Some(AccountsIndexConfig {
                index_limit_mb: IndexLimitMb::Limit(1),
                ..AccountsIndexConfig::default()
            }),
            1,
        ));
        let bin = 0;
        let bucket = InMemAccountsIndex::new(&holder, bin);
        assert!(bucket.storage.is_disk_index_enabled());
        bucket
    }

    #[test]
    fn test_should_evict_from_mem() {
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

        // exceeded budget
        assert!(
            bucket
                .should_evict_from_mem(
                    current_age,
                    &Arc::new(AccountMapEntryInner::new(
                        vec![],
                        ref_count,
                        AccountMapEntryMeta::default()
                    )),
                    startup,
                    false,
                    true,
                )
                .0
        );
        // empty slot list
        assert!(
            !bucket
                .should_evict_from_mem(
                    current_age,
                    &Arc::new(AccountMapEntryInner::new(
                        vec![],
                        ref_count,
                        AccountMapEntryMeta::default()
                    )),
                    startup,
                    false,
                    false,
                )
                .0
        );
        // 1 element slot list
        assert!(
            bucket
                .should_evict_from_mem(
                    current_age,
                    &one_element_slot_list_entry,
                    startup,
                    false,
                    false,
                )
                .0
        );
        // 2 element slot list
        assert!(
            !bucket
                .should_evict_from_mem(
                    current_age,
                    &Arc::new(AccountMapEntryInner::new(
                        vec![(0, 0), (1, 1)],
                        ref_count,
                        AccountMapEntryMeta::default()
                    )),
                    startup,
                    false,
                    false,
                )
                .0
        );

        {
            let bucket = new_for_test::<f64>();
            // 1 element slot list with a CACHED item - f64 acts like cached
            assert!(
                !bucket
                    .should_evict_from_mem(
                        current_age,
                        &Arc::new(AccountMapEntryInner::new(
                            vec![(0, 0.0)],
                            ref_count,
                            AccountMapEntryMeta::default()
                        )),
                        startup,
                        false,
                        false,
                    )
                    .0
            );
        }

        // 1 element slot list, age is now
        assert!(
            bucket
                .should_evict_from_mem(
                    current_age,
                    &one_element_slot_list_entry,
                    startup,
                    false,
                    false,
                )
                .0
        );

        // 1 element slot list, but not current age
        current_age = 1;
        assert!(
            !bucket
                .should_evict_from_mem(
                    current_age,
                    &one_element_slot_list_entry,
                    startup,
                    false,
                    false,
                )
                .0
        );

        // 1 element slot list, but at startup and age not current
        startup = true;
        assert!(
            bucket
                .should_evict_from_mem(
                    current_age,
                    &one_element_slot_list_entry,
                    startup,
                    false,
                    false,
                )
                .0
        );
    }

    #[test]
    fn test_hold_range_in_memory() {
        let bucket = new_disk_buckets_for_test::<u64>();
        // 0x81 is just some other range
        let all = Pubkey::new(&[0; 32])..=Pubkey::new(&[0xff; 32]);
        let ranges = [
            all.clone(),
            Pubkey::new(&[0x81; 32])..=Pubkey::new(&[0xff; 32]),
        ];
        for range in ranges.clone() {
            assert!(bucket.cache_ranges_held.read().unwrap().is_empty());
            bucket.hold_range_in_memory(&range, true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![range.clone()]
            );
            {
                let evictions_guard = EvictionsGuard::lock(&bucket);
                assert!(bucket.add_hold_range_in_memory_if_already_held(&range, &evictions_guard));
                bucket.hold_range_in_memory(&range, false);
            }
            bucket.hold_range_in_memory(&range, false);
            assert!(bucket.cache_ranges_held.read().unwrap().is_empty());
            bucket.hold_range_in_memory(&range, true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![range.clone()]
            );
            bucket.hold_range_in_memory(&range, true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![range.clone(), range.clone()]
            );
            bucket.hold_range_in_memory(&ranges[0], true);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![range.clone(), range.clone(), ranges[0].clone()]
            );
            bucket.hold_range_in_memory(&range, false);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![range.clone(), ranges[0].clone()]
            );
            bucket.hold_range_in_memory(&range, false);
            assert_eq!(
                bucket.cache_ranges_held.read().unwrap().to_vec(),
                vec![ranges[0].clone()]
            );
            bucket.hold_range_in_memory(&ranges[0].clone(), false);
            assert!(bucket.cache_ranges_held.read().unwrap().is_empty());

            // hold all in mem first
            assert!(bucket.cache_ranges_held.read().unwrap().is_empty());
            bucket.hold_range_in_memory(&all, true);

            let evictions_guard = EvictionsGuard::lock(&bucket);
            assert!(bucket.add_hold_range_in_memory_if_already_held(&range, &evictions_guard));
            bucket.hold_range_in_memory(&range, false);
            bucket.hold_range_in_memory(&all, false);
        }
    }

    #[test]
    fn test_age() {
        solana_logger::setup();
        let test = new_for_test::<u64>();
        assert!(test.get_should_age(test.storage.current_age()));
        assert_eq!(test.storage.count_buckets_flushed(), 0);
        test.set_has_aged(0, true);
        assert!(!test.get_should_age(test.storage.current_age()));
        assert_eq!(test.storage.count_buckets_flushed(), 1);
        // simulate rest of buckets aging
        for _ in 1..BINS_FOR_TESTING {
            assert!(!test.storage.all_buckets_flushed_at_current_age());
            test.storage.bucket_flushed_at_current_age(true);
        }
        assert!(test.storage.all_buckets_flushed_at_current_age());
        // advance age
        test.storage.increment_age();
        assert_eq!(test.storage.current_age(), 1);
        assert!(!test.storage.all_buckets_flushed_at_current_age());
        assert!(test.get_should_age(test.storage.current_age()));
        assert_eq!(test.storage.count_buckets_flushed(), 0);
    }

    #[test]
    fn test_update_slot_list_other() {
        solana_logger::setup();
        let reclaim = UpsertReclaim::PopulateReclaims;
        let new_slot = 0;
        let info = 1;
        let other_value = info + 1;
        let at_new_slot = (new_slot, info);
        let unique_other_slot = new_slot + 1;
        for other_slot in [Some(new_slot), Some(unique_other_slot), None] {
            let mut reclaims = Vec::default();
            let mut slot_list = Vec::default();
            // upserting into empty slot_list, so always addref
            assert!(
                InMemAccountsIndex::update_slot_list(
                    &mut slot_list,
                    new_slot,
                    info,
                    other_slot,
                    &mut reclaims,
                    reclaim
                ),
                "other_slot: {other_slot:?}"
            );
            assert_eq!(slot_list, vec![at_new_slot]);
            assert!(reclaims.is_empty());
        }

        // replace other
        let mut slot_list = vec![(unique_other_slot, other_value)];
        let expected_reclaims = slot_list.clone();
        let other_slot = Some(unique_other_slot);
        let mut reclaims = Vec::default();
        assert!(
            // upserting into slot_list that does NOT contain an entry at 'new_slot'
            // but, it DOES contain an entry at other_slot, so we do NOT add-ref. The assumption is that 'other_slot' is going away
            // and that the previously held add-ref is now used by 'new_slot'
            !InMemAccountsIndex::update_slot_list(
                &mut slot_list,
                new_slot,
                info,
                other_slot,
                &mut reclaims,
                reclaim
            ),
            "other_slot: {other_slot:?}"
        );
        assert_eq!(slot_list, vec![at_new_slot]);
        assert_eq!(reclaims, expected_reclaims);

        // replace other and new_slot
        let mut slot_list = vec![(unique_other_slot, other_value), (new_slot, other_value)];
        let expected_reclaims = slot_list.clone();
        let other_slot = Some(unique_other_slot);
        // upserting into slot_list that already contain an entry at 'new-slot', so do NOT addref
        let mut reclaims = Vec::default();
        assert!(
            !InMemAccountsIndex::update_slot_list(
                &mut slot_list,
                new_slot,
                info,
                other_slot,
                &mut reclaims,
                reclaim
            ),
            "other_slot: {other_slot:?}"
        );
        assert_eq!(slot_list, vec![at_new_slot]);
        assert_eq!(
            reclaims,
            expected_reclaims.into_iter().rev().collect::<Vec<_>>()
        );

        // nothing will exist at this slot
        let missing_other_slot = unique_other_slot + 1;
        let ignored_slot = 10; // bigger than is used elsewhere in the test
        let ignored_value = info + 10;

        let mut possible_initial_slot_list_contents;
        // build a list of possible contents in the slot_list prior to calling 'update_slot_list'
        {
            // up to 3 ignored slot account_info (ignored means not 'new_slot', not 'other_slot', but different slot #s which could exist in the slot_list initially)
            possible_initial_slot_list_contents = (0..3)
                .map(|i| (ignored_slot + i, ignored_value + i))
                .collect::<Vec<_>>();
            // account_info that already exists in the slot_list AT 'new_slot'
            possible_initial_slot_list_contents.push(at_new_slot);
            // account_info that already exists in the slot_list AT 'other_slot'
            possible_initial_slot_list_contents.push((unique_other_slot, other_value));
        }

        /*
         * loop over all possible permutations of 'possible_initial_slot_list_contents'
         * some examples:
         * []
         * [other]
         * [other, new_slot]
         * [new_slot, other]
         * [dummy0, new_slot, dummy1, other] (and all permutation of this order)
         * [other, dummy1, new_slot] (and all permutation of this order)
         * ...
         * [dummy0, new_slot, dummy1, other_slot, dummy2] (and all permutation of this order)
         */
        let mut attempts = 0;
        // loop over each initial size of 'slot_list'
        for initial_slot_list_len in 0..=possible_initial_slot_list_contents.len() {
            // loop over every permutation of possible_initial_slot_list_contents within a list of len 'initial_slot_list_len'
            for content_source_indexes in
                (0..possible_initial_slot_list_contents.len()).permutations(initial_slot_list_len)
            {
                // loop over each possible parameter for 'other_slot'
                for other_slot in [
                    Some(new_slot),
                    Some(unique_other_slot),
                    Some(missing_other_slot),
                    None,
                ] {
                    attempts += 1;
                    // initialize slot_list prior to call to 'InMemAccountsIndex::update_slot_list'
                    // by inserting each possible entry at each possible position
                    let mut slot_list = content_source_indexes
                        .iter()
                        .map(|i| possible_initial_slot_list_contents[*i])
                        .collect::<Vec<_>>();
                    let mut expected = slot_list.clone();
                    let original = slot_list.clone();
                    let mut reclaims = Vec::default();

                    let result = InMemAccountsIndex::update_slot_list(
                        &mut slot_list,
                        new_slot,
                        info,
                        other_slot,
                        &mut reclaims,
                        reclaim,
                    );

                    // calculate expected results
                    let mut expected_reclaims = Vec::default();
                    // addref iff the slot_list did NOT previously contain an entry at 'new_slot' and it also did not contain an entry at 'other_slot'
                    let expected_result = !expected
                        .iter()
                        .any(|(slot, _info)| slot == &new_slot || Some(*slot) == other_slot);
                    {
                        // this is the logical equivalent of 'InMemAccountsIndex::update_slot_list', but slower (and ignoring addref)
                        expected.retain(|(slot, info)| {
                            let retain = slot != &new_slot && Some(*slot) != other_slot;
                            if !retain {
                                expected_reclaims.push((*slot, *info));
                            }
                            retain
                        });
                        expected.push((new_slot, info));
                    }
                    assert_eq!(
                        expected_result, result,
                        "return value different. other: {other_slot:?}, {expected:?}, {slot_list:?}, original: {original:?}"
                    );
                    // sort for easy comparison
                    expected_reclaims.sort_unstable();
                    reclaims.sort_unstable();
                    assert_eq!(
                        expected_reclaims, reclaims,
                        "reclaims different. other: {other_slot:?}, {expected:?}, {slot_list:?}, original: {original:?}"
                    );
                    // sort for easy comparison
                    slot_list.sort_unstable();
                    expected.sort_unstable();
                    assert_eq!(
                        slot_list, expected,
                        "slot_list different. other: {other_slot:?}, {expected:?}, {slot_list:?}, original: {original:?}"
                    );
                }
            }
        }
        assert_eq!(attempts, 1304); // complicated permutations, so make sure we ran the right #
    }

    #[test]
    fn test_flush_guard() {
        let flushing_active = AtomicBool::new(false);

        {
            let flush_guard = FlushGuard::lock(&flushing_active);
            assert!(flush_guard.is_some());
            assert!(flushing_active.load(Ordering::Acquire));

            {
                // Trying to lock the FlushGuard again will not succeed.
                let flush_guard2 = FlushGuard::lock(&flushing_active);
                assert!(flush_guard2.is_none());
            }

            // The `flushing_active` flag will remain true, even after `flush_guard2` goes out of
            // scope (and is dropped).  This ensures `lock()` and `drop()` work harmoniously.
            assert!(flushing_active.load(Ordering::Acquire));
        }

        // After the FlushGuard is dropped, the flag will be cleared.
        assert!(!flushing_active.load(Ordering::Acquire));
    }

    #[test]
    fn test_remove_if_slot_list_empty_entry() {
        let key = solana_sdk::pubkey::new_rand();
        let unknown_key = solana_sdk::pubkey::new_rand();

        let test = new_for_test::<u64>();

        let mut map = test.map_internal.write().unwrap();

        {
            // item is NOT in index at all, still return true from remove_if_slot_list_empty_entry
            // make sure not initially in index
            let entry = map.entry(unknown_key);
            assert!(matches!(entry, Entry::Vacant(_)));
            let entry = map.entry(unknown_key);
            assert!(test.remove_if_slot_list_empty_entry(entry));
            // make sure still not in index
            let entry = map.entry(unknown_key);
            assert!(matches!(entry, Entry::Vacant(_)));
        }

        {
            // add an entry with an empty slot list
            let val = Arc::new(AccountMapEntryInner::<u64>::default());
            map.insert(key, val);
            let entry = map.entry(key);
            assert!(matches!(entry, Entry::Occupied(_)));
            // should have removed it since it had an empty slot list
            assert!(test.remove_if_slot_list_empty_entry(entry));
            let entry = map.entry(key);
            assert!(matches!(entry, Entry::Vacant(_)));
            // return true - item is not in index at all now
            assert!(test.remove_if_slot_list_empty_entry(entry));
        }

        {
            // add an entry with a NON empty slot list - it will NOT get removed
            let val = Arc::new(AccountMapEntryInner::<u64>::default());
            val.slot_list.write().unwrap().push((1, 1));
            map.insert(key, val);
            // does NOT remove it since it has a non-empty slot list
            let entry = map.entry(key);
            assert!(!test.remove_if_slot_list_empty_entry(entry));
            let entry = map.entry(key);
            assert!(matches!(entry, Entry::Occupied(_)));
        }
    }

    #[test]
    fn test_lock_and_update_slot_list() {
        let test = AccountMapEntryInner::<u64>::default();
        let info = 65;
        let mut reclaims = Vec::default();
        // first upsert, should increase
        let len = InMemAccountsIndex::lock_and_update_slot_list(
            &test,
            (1, info),
            None,
            &mut reclaims,
            UpsertReclaim::IgnoreReclaims,
        );
        assert_eq!(test.slot_list.read().unwrap().len(), len);
        assert_eq!(len, 1);
        // update to different slot, should increase
        let len = InMemAccountsIndex::lock_and_update_slot_list(
            &test,
            (2, info),
            None,
            &mut reclaims,
            UpsertReclaim::IgnoreReclaims,
        );
        assert_eq!(test.slot_list.read().unwrap().len(), len);
        assert_eq!(len, 2);
        // update to same slot, should not increase
        let len = InMemAccountsIndex::lock_and_update_slot_list(
            &test,
            (2, info),
            None,
            &mut reclaims,
            UpsertReclaim::IgnoreReclaims,
        );
        assert_eq!(test.slot_list.read().unwrap().len(), len);
        assert_eq!(len, 2);
    }
}
