use {
    crate::{
        accounts_index::{
            AccountMapEntry, AccountMapEntryInner, AccountMapEntryMeta, IndexValue,
            PreAllocatedAccountMapEntry, RefCount, SlotList, SlotSlice, ZeroLamport,
        },
        bucket_map_holder::{Age, BucketMapHolder},
        bucket_map_holder_stats::BucketMapHolderStats,
    },
    rand::{thread_rng, Rng},
    solana_bucket_map::bucket_api::BucketApi,
    solana_measure::measure::Measure,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{
        collections::{
            hash_map::{Entry, VacantEntry},
            HashMap,
        },
        fmt::Debug,
        ops::{Bound, RangeBounds, RangeInclusive},
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
            Arc, RwLock, RwLockWriteGuard,
        },
    },
};
type K = Pubkey;
type CacheRangesHeld = RwLock<Vec<RangeInclusive<Pubkey>>>;
pub type SlotT<T> = (Slot, T);

type InMemMap<T> = HashMap<Pubkey, AccountMapEntry<T>>;

#[allow(dead_code)] // temporary during staging
                    // one instance of this represents one bin of the accounts index.
pub struct InMemAccountsIndex<T: IndexValue> {
    last_age_flushed: AtomicU8,

    // backing store
    map_internal: RwLock<InMemMap<T>>,
    storage: Arc<BucketMapHolder<T>>,
    bin: usize,

    bucket: Option<Arc<BucketApi<SlotT<T>>>>,

    // pubkey ranges that this bin must hold in the cache while the range is present in this vec
    pub(crate) cache_ranges_held: CacheRangesHeld,
    // incremented each time stop_evictions is changed
    stop_evictions_changes: AtomicU64,
    // true while ranges are being manipulated. Used to keep an async flush from removing things while a range is being held.
    stop_evictions: AtomicU64,
    // set to true while this bin is being actively flushed
    flushing_active: AtomicBool,
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
            stop_evictions_changes: AtomicU64::default(),
            stop_evictions: AtomicU64::default(),
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
        let map = self.map().read().unwrap();
        let mut result = Vec::with_capacity(map.len());
        map.iter().for_each(|(k, v)| {
            if range.contains(k) {
                result.push((*k, Arc::clone(v)));
            }
        });
        self.hold_range_in_memory(range, false);
        Self::update_stat(&self.stats().items, 1);
        Self::update_time_stat(&self.stats().items_us, m);
        result
    }

    // only called in debug code paths
    pub fn keys(&self) -> Vec<Pubkey> {
        Self::update_stat(&self.stats().keys, 1);
        // easiest implementation is to load evrything from disk into cache and return the keys
        self.start_stop_evictions(true);
        self.put_range_in_cache(&None::<&RangeInclusive<Pubkey>>);
        let keys = self.map().read().unwrap().keys().cloned().collect();
        self.start_stop_evictions(false);
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
        callback: impl for<'a> FnOnce(Option<&'a AccountMapEntry<T>>) -> RT,
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
        self.get_internal(pubkey, |entry| (true, entry.map(Arc::clone)))
    }

    /// lookup 'pubkey' in index (in_mem or disk).
    /// call 'callback' whether found or not
    pub(crate) fn get_internal<RT>(
        &self,
        pubkey: &K,
        // return true if item should be added to in_mem cache
        callback: impl for<'a> FnOnce(Option<&AccountMapEntry<T>>) -> (bool, RT),
    ) -> RT {
        self.get_only_in_mem(pubkey, |entry| {
            if let Some(entry) = entry {
                entry.set_age(self.storage.future_age_to_flush());
                callback(Some(entry)).1
            } else {
                // not in cache, look on disk
                let stats = &self.stats();
                let disk_entry = self.load_account_entry_from_disk(pubkey);
                if disk_entry.is_none() {
                    return callback(None).1;
                }
                let disk_entry = disk_entry.unwrap();
                let mut map = self.map().write().unwrap();
                let entry = map.entry(*pubkey);
                match entry {
                    Entry::Occupied(occupied) => callback(Some(occupied.get())).1,
                    Entry::Vacant(vacant) => {
                        let (add_to_cache, rt) = callback(Some(&disk_entry));

                        if add_to_cache {
                            stats.insert_or_delete_mem(true, self.bin);
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
        let mut m = Measure::start("entry");
        let mut map = self.map().write().unwrap();
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

    pub fn unref(&self, pubkey: &Pubkey) {
        self.get_internal(pubkey, |entry| {
            if let Some(entry) = entry {
                entry.add_un_ref(false)
            }
            (true, ())
        })
    }

    pub fn upsert(
        &self,
        pubkey: &Pubkey,
        new_value: PreAllocatedAccountMapEntry<T>,
        other_slot: Option<Slot>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) {
        // try to get it just from memory first using only a read lock
        self.get_only_in_mem(pubkey, |entry| {
            if let Some(entry) = entry {
                Self::lock_and_update_slot_list(
                    entry,
                    new_value.into(),
                    other_slot,
                    reclaims,
                    previous_slot_entry_was_cached,
                );
                Self::update_stat(&self.stats().updates_in_mem, 1);
            } else {
                let mut m = Measure::start("entry");
                let mut map = self.map().write().unwrap();
                let entry = map.entry(*pubkey);
                m.stop();
                let found = matches!(entry, Entry::Occupied(_));
                match entry {
                    Entry::Occupied(mut occupied) => {
                        let current = occupied.get_mut();
                        Self::lock_and_update_slot_list(
                            current,
                            new_value.into(),
                            other_slot,
                            reclaims,
                            previous_slot_entry_was_cached,
                        );
                        current.set_age(self.storage.future_age_to_flush());
                        Self::update_stat(&self.stats().updates_in_mem, 1);
                    }
                    Entry::Vacant(vacant) => {
                        // not in cache, look on disk

                        // desired to be this for filler accounts: self.storage.get_startup();
                        // but, this has proven to be far too slow at high account counts
                        let directly_to_disk = false;
                        if directly_to_disk {
                            // We may like this to always run, but it is unclear.
                            // If disk bucket needs to resize, then this call can stall for a long time.
                            // Right now, we know it is safe during startup.
                            let already_existed = self.upsert_on_disk(
                                vacant,
                                new_value,
                                other_slot,
                                reclaims,
                                previous_slot_entry_was_cached,
                            );
                            if !already_existed {
                                self.stats().insert_or_delete(true, self.bin);
                            }
                        } else {
                            // go to in-mem cache first
                            let disk_entry = self.load_account_entry_from_disk(vacant.key());
                            let new_value = if let Some(disk_entry) = disk_entry {
                                // on disk, so merge new_value with what was on disk
                                Self::lock_and_update_slot_list(
                                    &disk_entry,
                                    new_value.into(),
                                    other_slot,
                                    reclaims,
                                    previous_slot_entry_was_cached,
                                );
                                disk_entry
                            } else {
                                // not on disk, so insert new thing
                                self.stats().insert_or_delete(true, self.bin);
                                new_value.into_account_map_entry(&self.storage)
                            };
                            assert!(new_value.dirty());
                            vacant.insert(new_value);
                            self.stats().insert_or_delete_mem(true, self.bin);
                        }
                    }
                }

                drop(map);
                self.update_entry_stats(m, found);
            };
        })
    }

    fn update_entry_stats(&self, stopped_measure: Measure, found: bool) {
        let stats = &self.stats();
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
    pub fn lock_and_update_slot_list(
        current: &AccountMapEntryInner<T>,
        new_value: (Slot, T),
        other_slot: Option<Slot>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) {
        let mut slot_list = current.slot_list.write().unwrap();
        let (slot, new_entry) = new_value;
        let addref = Self::update_slot_list(
            &mut slot_list,
            slot,
            new_entry,
            other_slot,
            reclaims,
            previous_slot_entry_was_cached,
        );
        if addref {
            current.add_un_ref(true);
        }
        current.set_dirty(true);
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
        previous_slot_entry_was_cached: bool,
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
            .into_iter()
            .rev() // rev since we delete from the list in some cases
            .for_each(|slot_list_index| {
                let (cur_slot, cur_account_info) = &slot_list[slot_list_index];
                let matched_slot = *cur_slot == slot;
                if matched_slot || Some(*cur_slot) == other_slot {
                    // make sure neither 'slot' nor 'other_slot' are in the slot list more than once
                    let matched_other_slot = !matched_slot;
                    assert!(
                        !(found_slot && matched_slot || matched_other_slot && found_other_slot),
                        "{:?}, slot: {}, other_slot: {:?}",
                        slot_list,
                        slot,
                        other_slot
                    );

                    let is_cur_account_cached = cur_account_info.is_cached();

                    let reclaim_item = if !(found_slot || found_other_slot) {
                        // first time we found an entry in 'slot' or 'other_slot', so replace it in-place.
                        // this may be the only instance we find
                        let mut new_item = (slot, account_info);
                        std::mem::swap(&mut new_item, &mut slot_list[slot_list_index]);
                        new_item
                    } else {
                        // already replaced one entry, so this one has to be removed
                        slot_list.remove(slot_list_index)
                    };
                    if previous_slot_entry_was_cached {
                        assert!(is_cur_account_cached);
                    } else {
                        reclaims.push(reclaim_item);
                    }

                    if matched_slot {
                        found_slot = true;
                        if !is_cur_account_cached {
                            // current info at 'slot' is NOT cached, so we should NOT addref. This slot already has a ref count for this pubkey.
                            addref = false;
                        }
                    } else {
                        found_other_slot = true;
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

    pub fn insert_new_entry_if_missing_with_lock(
        &self,
        pubkey: Pubkey,
        new_entry: PreAllocatedAccountMapEntry<T>,
    ) -> InsertNewEntryResults {
        let mut m = Measure::start("entry");
        let mut map = self.map().write().unwrap();
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
                    false,
                );
                (
                    true, /* found in mem */
                    true, /* already existed */
                )
            }
            Entry::Vacant(vacant) => {
                // not in cache, look on disk
                let initial_insert_directly_to_disk = false;
                if initial_insert_directly_to_disk {
                    // This is more direct, but becomes too slow with very large acct #.
                    // disk buckets will be improved to make them more performant. Tuning the disks may also help.
                    // This may become a config tuning option.
                    let already_existed = self.upsert_on_disk(
                        vacant,
                        new_entry,
                        None, // not changing slots here since it doesn't exist in the index at all
                        &mut Vec::default(),
                        false,
                    );
                    (false, already_existed)
                } else {
                    let disk_entry = self.load_account_entry_from_disk(vacant.key());
                    self.stats().insert_or_delete_mem(true, self.bin);
                    if let Some(disk_entry) = disk_entry {
                        let (slot, account_info) = new_entry.into();
                        InMemAccountsIndex::lock_and_update_slot_list(
                            &disk_entry,
                            (slot, account_info),
                            // None because we are inserting the first element in the slot list for this pubkey.
                            // There can be no 'other' slot in the list.
                            None,
                            &mut Vec::default(),
                            false,
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
            }
        };
        drop(map);
        self.update_entry_stats(m, found_in_mem);
        let stats = self.stats();
        if !already_existed {
            stats.insert_or_delete(true, self.bin);
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

    /// return true if item already existed in the index
    fn upsert_on_disk(
        &self,
        vacant: VacantEntry<K, AccountMapEntry<T>>,
        new_entry: PreAllocatedAccountMapEntry<T>,
        other_slot: Option<Slot>,
        reclaims: &mut SlotList<T>,
        previous_slot_entry_was_cached: bool,
    ) -> bool {
        if let Some(disk) = self.bucket.as_ref() {
            let mut existed = false;
            let (slot, account_info) = new_entry.into();
            disk.update(vacant.key(), |current| {
                if let Some((slot_list, mut ref_count)) = current {
                    // on disk, so merge and update disk
                    let mut slot_list = slot_list.to_vec();
                    let addref = Self::update_slot_list(
                        &mut slot_list,
                        slot,
                        account_info,
                        other_slot,
                        reclaims,
                        previous_slot_entry_was_cached,
                    );
                    if addref {
                        ref_count += 1
                    };
                    existed = true; // found on disk, so it did exist
                    Some((slot_list, ref_count))
                } else {
                    // doesn't exist on disk yet, so insert it
                    let ref_count = if account_info.is_cached() { 0 } else { 1 };
                    Some((vec![(slot, account_info)], ref_count))
                }
            });
            existed
        } else {
            // not using disk, so insert into mem
            self.stats().insert_or_delete_mem(true, self.bin);
            let new_entry: AccountMapEntry<T> = new_entry.into_account_map_entry(&self.storage);
            assert!(new_entry.dirty());
            vacant.insert(new_entry);
            false // not using disk, not in mem, so did not exist
        }
    }

    /// Look at the currently held ranges. If 'range' is already included in what is
    ///  being held, then add 'range' to the currently held list AND return true
    /// If 'range' is NOT already included in what is being held, then return false
    ///  withOUT adding 'range' to the list of what is currently held
    fn add_hold_range_in_memory_if_already_held<R>(&self, range: &R) -> bool
    where
        R: RangeBounds<Pubkey>,
    {
        let start_holding = true;
        let only_add_if_already_held = true;
        self.just_set_hold_range_in_memory_internal(range, start_holding, only_add_if_already_held)
    }

    fn just_set_hold_range_in_memory<R>(&self, range: &R, start_holding: bool)
    where
        R: RangeBounds<Pubkey>,
    {
        let only_add_if_already_held = false;
        let _ = self.just_set_hold_range_in_memory_internal(
            range,
            start_holding,
            only_add_if_already_held,
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

    /// called with 'stop'=true to stop bg flusher from evicting any entries from in-mem idx
    /// called with 'stop'=false to allow bg flusher to evict eligible (not in held ranges) entries from in-mem idx
    fn start_stop_evictions(&self, stop: bool) {
        if stop {
            self.stop_evictions.fetch_add(1, Ordering::Release);
        } else if 1 == self.stop_evictions.fetch_sub(1, Ordering::Release) {
            // stop_evictions went to 0, so this bucket could now be ready to be aged
            self.storage.wait_dirty_or_aged.notify_one();
        }
        // note that this value has changed
        self.stop_evictions_changes.fetch_add(1, Ordering::Release);
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
        self.start_stop_evictions(true);

        if !start_holding || !self.add_hold_range_in_memory_if_already_held(range) {
            if start_holding {
                // put everything in the cache and it will be held there
                self.put_range_in_cache(&Some(range));
            }
            // do this AFTER items have been put in cache - that way anyone who finds this range can know that the items are already in the cache
            self.just_set_hold_range_in_memory(range, start_holding);
        }

        self.start_stop_evictions(false);
    }

    fn put_range_in_cache<R>(&self, range: &Option<&R>)
    where
        R: RangeBounds<Pubkey>,
    {
        assert!(self.get_stop_evictions()); // caller should be controlling the lifetime of how long this needs to be present
        let m = Measure::start("range");

        let mut added_to_mem = 0;
        // load from disk
        if let Some(disk) = self.bucket.as_ref() {
            let mut map = self.map().write().unwrap();
            let items = disk.items_in_range(range); // map's lock has to be held while we are getting items from disk
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
                        added_to_mem += 1;
                    }
                }
            }
        }
        self.stats()
            .insert_or_delete_mem_count(true, self.bin, added_to_mem);

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

    pub(crate) fn flush(&self) {
        if let Some(flush_guard) = FlushGuard::lock(&self.flushing_active) {
            self.flush_internal(&flush_guard)
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

    fn flush_internal(&self, _flush_guard: &FlushGuard) {
        let current_age = self.storage.current_age();
        let mut iterate_for_age = self.get_should_age(current_age);
        let startup = self.storage.get_startup();
        if !iterate_for_age && !startup {
            // no need to age, so no need to flush this bucket
            // but, at startup we want to remove from buckets as fast as possible if any items exist
            return;
        }

        let in_mem_count = self.stats().count_in_mem.load(Ordering::Relaxed);
        let limit = self.storage.mem_budget_mb;
        let estimate_mem = in_mem_count * Self::approx_size_of_one_entry();
        let exceeds_budget = limit
            .map(|limit| estimate_mem >= limit * 1024 * 1024)
            .unwrap_or_default();
        self.stats()
            .estimate_mem
            .store(estimate_mem as u64, Ordering::Relaxed);

        // may have to loop if disk has to grow and we have to restart
        loop {
            let mut evictions;
            let mut evictions_random = Vec::default();
            let disk = self.bucket.as_ref().unwrap();

            let mut flush_entries_updated_on_disk = 0;
            let mut disk_resize = Ok(());
            let mut flush_should_evict_us = 0;
            // scan and update loop
            // holds read lock
            {
                let map = self.map().read().unwrap();
                evictions = Vec::with_capacity(map.len());
                let m = Measure::start("flush_scan_and_update"); // we don't care about lock time in this metric - bg threads can wait
                for (k, v) in map.iter() {
                    let mut mse = Measure::start("flush_should_evict");
                    let (evict_for_age, slot_list) =
                        self.should_evict_from_mem(current_age, v, startup, true, exceeds_budget);
                    mse.stop();
                    flush_should_evict_us += mse.as_us();
                    if !evict_for_age && !Self::random_chance_of_eviction() {
                        // not planning to remove this item from memory now, so don't write it to disk yet
                        continue;
                    }

                    // if we are removing it, then we need to update disk if we're dirty
                    if v.clear_dirty() {
                        // step 1: clear the dirty flag
                        // step 2: perform the update on disk based on the fields in the entry
                        // If a parallel operation dirties the item again - even while this flush is occurring,
                        //  the last thing the writer will do, after updating contents, is set_dirty(true)
                        //  That prevents dropping an item from cache before disk is updated to latest in mem.
                        // happens inside of lock on in-mem cache. This is because of deleting items
                        // it is possible that the item in the cache is marked as dirty while these updates are happening. That is ok.
                        {
                            let slot_list =
                                slot_list.unwrap_or_else(|| v.slot_list.read().unwrap());
                            disk_resize = disk.try_write(k, (&slot_list, v.ref_count()));
                        }
                        if disk_resize.is_ok() {
                            flush_entries_updated_on_disk += 1;
                        } else {
                            // disk needs to resize, so mark all unprocessed items as dirty again so we pick them up after the resize
                            v.set_dirty(true);
                            break;
                        }
                    } else {
                        drop(slot_list);
                    }
                    if evict_for_age {
                        evictions.push(*k);
                    } else {
                        evictions_random.push(*k);
                    }
                }
                Self::update_time_stat(&self.stats().flush_scan_update_us, m);
            }
            Self::update_stat(&self.stats().flush_should_evict_us, flush_should_evict_us);

            Self::update_stat(
                &self.stats().flush_entries_updated_on_disk,
                flush_entries_updated_on_disk,
            );

            let m = Measure::start("flush_evict_or_grow");
            match disk_resize {
                Ok(_) => {
                    if !self.evict_from_cache(evictions, current_age, startup, false)
                        || !self.evict_from_cache(evictions_random, current_age, startup, true)
                    {
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
                    Self::update_time_stat(&self.stats().flush_grow_us, m);
                }
            }
        }
    }

    // remove keys in 'evictions' from in-mem cache, likely due to age
    // return true if the removal was completed
    fn evict_from_cache(
        &self,
        mut evictions: Vec<Pubkey>,
        current_age: Age,
        startup: bool,
        randomly_evicted: bool,
    ) -> bool {
        let mut completed_scan = true;
        if evictions.is_empty() {
            return completed_scan; // completed, don't need to get lock or do other work
        }

        let stop_evictions_changes_at_start = self.get_stop_evictions_changes();
        if self.get_stop_evictions() {
            return false; // did NOT complete, ranges were changed, so have to restart
        }

        // skip any keys that are held in memory because of ranges being held
        let ranges = self.cache_ranges_held.read().unwrap().clone();
        if !ranges.is_empty() {
            evictions.retain(|k| {
                if ranges.iter().any(|range| range.contains(k)) {
                    // this item is held in mem by range, so don't remove
                    completed_scan = false;
                    false
                } else {
                    true
                }
            });
        }

        let mut removed = 0;
        // consider chunking these so we don't hold the write lock too long
        let mut map = self.map().write().unwrap();
        for k in evictions {
            if let Entry::Occupied(occupied) = map.entry(k) {
                let v = occupied.get();
                if Arc::strong_count(v) > 1 {
                    // someone is holding the value arc's ref count and could modify it, so do not remove this from in-mem cache
                    completed_scan = false;
                    continue;
                }

                if v.dirty()
                    || (!randomly_evicted
                        && !Self::should_evict_based_on_age(current_age, v, startup))
                {
                    // marked dirty or bumped in age after we looked above
                    // these flushes will be handled in later passes (at later ages)
                    // but, at startup, everything is ready to age out if it isn't dirty
                    continue;
                }

                if stop_evictions_changes_at_start != self.get_stop_evictions_changes() {
                    return false; // did NOT complete, ranges were changed, so have to restart
                }

                // all conditions for removing succeeded, so really remove item from in-mem cache
                removed += 1;
                occupied.remove();
            }
        }
        if map.is_empty() {
            map.shrink_to_fit();
        }
        drop(map);
        self.stats()
            .insert_or_delete_mem_count(false, self.bin, removed);
        Self::update_stat(&self.stats().flush_entries_removed_from_mem, removed as u64);

        completed_scan
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
        (!already_flushing).then(|| Self { flushing })
    }
}

impl Drop for FlushGuard<'_> {
    fn drop(&mut self) {
        self.flushing.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::accounts_index::{AccountsIndexConfig, BINS_FOR_TESTING},
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
                index_limit_mb: Some(1),
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
                assert!(bucket.add_hold_range_in_memory_if_already_held(&range));
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
            assert!(bucket.add_hold_range_in_memory_if_already_held(&range));
            bucket.hold_range_in_memory(&range, false);
            bucket.hold_range_in_memory(&all, false);
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

    #[test]
    fn test_update_slot_list_other() {
        solana_logger::setup();
        let previous_slot_entry_was_cached = false;
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
                    previous_slot_entry_was_cached
                ),
                "other_slot: {:?}",
                other_slot
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
            // upserting into slot_list that does NOT contain an entry at 'new-slot', so always addref
            InMemAccountsIndex::update_slot_list(
                &mut slot_list,
                new_slot,
                info,
                other_slot,
                &mut reclaims,
                previous_slot_entry_was_cached
            ),
            "other_slot: {:?}",
            other_slot
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
                previous_slot_entry_was_cached
            ),
            "other_slot: {:?}",
            other_slot
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
                .into_iter()
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
                        previous_slot_entry_was_cached,
                    );

                    // calculate expected results
                    let mut expected_reclaims = Vec::default();
                    // addref iff the slot_list did NOT previously contain an entry at 'new_slot'
                    let expected_result = !expected.iter().any(|(slot, _info)| slot == &new_slot);
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
                        "return value different. other: {:?}, {:?}, {:?}, original: {:?}",
                        other_slot, expected, slot_list, original
                    );
                    // sort for easy comparison
                    expected_reclaims.sort_unstable();
                    reclaims.sort_unstable();
                    assert_eq!(
                        expected_reclaims, reclaims,
                        "reclaims different. other: {:?}, {:?}, {:?}, original: {:?}",
                        other_slot, expected, slot_list, original
                    );
                    // sort for easy comparison
                    slot_list.sort_unstable();
                    expected.sort_unstable();
                    assert_eq!(
                        slot_list, expected,
                        "slot_list different. other: {:?}, {:?}, {:?}, original: {:?}",
                        other_slot, expected, slot_list, original
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
}
