use crate::accounts_index::{AccountMapEntry, AccountMapEntryInner};
use crate::accounts_index::{IsCached, RefCount, SlotList, SlotSlice, WriteAccountMapEntry};
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::in_mem_accounts_index::{SlotT, V2};
use crate::pubkey_bins::PubkeyBinCalculator16;
use rand::{thread_rng, Rng};
use solana_bucket_map::bucket_map::{BucketMap, BucketMapKeyValue};
use solana_measure::measure::Measure;
use solana_sdk::pubkey::Pubkey;
use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::ops::{Bound, RangeBounds, RangeInclusive};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::{RwLock, RwLockWriteGuard};
use std::time::Instant;
use log::*;
pub type K = Pubkey;

use std::collections::{hash_map::Entry as HashMapEntry, HashMap, HashSet};
use std::convert::TryInto;

pub type WriteCache<V> = HashMap<Pubkey, V, MyBuildHasher>;
use crate::waitable_condvar::WaitableCondvar;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8};
use std::time::Duration;

use std::hash::{BuildHasherDefault, Hasher};
#[derive(Debug, Default)]
pub struct MyHasher {
    hash: u64,
}

pub type BucketMapWithEntryType<V> = BucketMap<SlotT<V>>;

type CacheWriteLock<'a, T> = RwLockWriteGuard<'a, WriteCache<V2<T>>>;

pub const AGE_MS: usize = 400; // # of ms for age to advance by 1

// When something is intending to remain in the cache for a while, then:WriteCacheEntryArc
//  add current age + DEFAULT_AGE_INCREMENT to specify when this item should be thrown out of the cache.
//  Example: setting DEFAULT_AGE_INCREMENT to 10 would specify that a new entry in the cache would remain for
//   10 * 400ms = ~4s.
//  Note that if the bg flusher thread is too busy, the reporting will not be on time.
pub const DEFAULT_AGE_INCREMENT: u8 = 25;
pub const VERIFY_GET_ON_INSERT: bool = false;
pub const TRY_READ_LOCKS_FIRST: bool = true;

impl Hasher for MyHasher {
    fn write(&mut self, bytes: &[u8]) {
        let len = bytes.len();
        let start = len - 8;
        self.hash ^= u64::from_be_bytes(bytes[start..].try_into().unwrap());
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}
type MyBuildHasher = BuildHasherDefault<MyHasher>;

type WriteCacheEntryArc<V> = AccountMapEntry<V>;
type WriteCacheEntry<V> = AccountMapEntryInner<V>;

pub type Cache<V> = Vec<CacheBin<V>>;
pub type CacheBin<V> = RwLock<WriteCache<WriteCacheEntryArc<V>>>;
pub type CacheSlice<'a, V> = &'a [CacheBin<V>];

#[derive(Debug)]
pub struct BucketMapHolder<V: IsCached> {
    pub current_age: AtomicU8,
    pub disk: BucketMapWithEntryType<V>,
    pub cache: Cache<V>,
    pub cache_ranges_held: Vec<RwLock<Vec<Option<RangeInclusive<Pubkey>>>>>,
    pub stop_flush: Vec<AtomicU64>,
    pub startup: AtomicBool,

    pub bins: usize,
    pub wait: WaitableCondvar,
    binner: PubkeyBinCalculator16,
    pub in_mem_only: bool,
    pub range_start_per_bin: Vec<Pubkey>,
    pub stats: BucketMapHolderStats,
}

impl<V: IsCached> BucketMapHolder<V> {
    pub fn update_or_insert_async(&self, ix: usize, key: Pubkey, new_value: AccountMapEntry<V>) {
        // if in cache, update now in cache
        // if known not present, insert into cache
        // if not in cache, put in cache, with flag saying we need to update later
        let wc = &mut self.cache[ix].write().unwrap();
        let m1 = Measure::start("update_or_insert_async");
        let res = wc.entry(key);
        match res {
            HashMapEntry::Occupied(occupied) => {
                // already in cache, so call update_static function
                let previous_slot_entry_was_cached = true;
                let mut reclaims = Vec::default();
                // maybe this, except this case is exactly what we're trying to avoid for perf...self.get_caching(occupied, key); // updates from disk if necessary
                self.upsert_item_in_cache(
                    occupied.get(),
                    new_value,
                    &mut reclaims,
                    previous_slot_entry_was_cached,
                    m1,
                );
            }
            HashMapEntry::Vacant(vacant) => {
                let must_do_lookup_from_disk = true;
                // not in cache - may or may not be on disk
                new_value.set_dirty(true);
                new_value.set_insert(true);
                new_value.set_must_do_lookup_from_disk(must_do_lookup_from_disk);
                assert!(!new_value.confirmed_not_on_disk());
                vacant.insert(new_value);
                self.wait.notify_all();
            }
        }
    }

    // cache does not change in this function
    pub fn just_set_hold_range_in_memory<R>(&self, ix: usize, range: &R, hold: bool)
    where
        R: RangeBounds<Pubkey>,
    {
        let start = match range.start_bound() {
            Bound::Included(bound) | Bound::Excluded(bound) => *bound,
            Bound::Unbounded => Pubkey::new(&[0; 32]),
        };

        let full_range = start <= self.range_start_per_bin[ix];

        let end = match range.end_bound() {
            Bound::Included(bound) | Bound::Excluded(bound) => *bound,
            Bound::Unbounded => Pubkey::new(&[0xff; 32]),
        };
        // off by 1 error here - if range is next_bin_pubkey-1 bit, then we are still full range
        // this just means that the bin will not treat a range of start..=(next_bin_pubkey-1 bit) as a full/None range.
        // We could fix this. It could cause an extra full disk scan of the bucket. It is an optimization.
        let inclusive_range = if full_range && end >= self.range_start_per_bin[ix + 1] {
            None
        } else {
            Some(start..=end)
        };
        let mut ranges = self.cache_ranges_held[ix].write().unwrap();
        if hold {
            ranges.push(inclusive_range);
        } else {
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
                // found a match
                ranges.remove(i);
                break;
            }
        }
    }

    fn start_stop_flush(&self, ix: usize, stop: bool) {
        if stop {
            self.stop_flush[ix].fetch_add(1, Ordering::Relaxed);
        } else {
            self.stop_flush[ix].fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn hold_range_in_memory<R>(&self, ix: usize, range: &R, hold: bool)
    where
        R: RangeBounds<Pubkey>,
    {
        self.start_stop_flush(ix, true);

        if hold {
            // put everything in the cache and it will be held there
            self.put_range_in_cache(ix, Some(range));
        }
        // do this AFTER items have been put in cache - that way anyone who finds this range can know that the items are already in the cache
        self.just_set_hold_range_in_memory(ix, range, hold);

        self.start_stop_flush(ix, false);
    }

    pub fn get_write(&self, index: usize) -> CacheWriteLock<V> {
        self.cache[index].write().unwrap()
    }

    fn add_age(age: u8, inc: u8) -> u8 {
        age.wrapping_add(inc)
    }

    pub fn bg_flusher(&self, exit: Arc<AtomicBool>, exit_when_idle: bool) {
        let mut found_one = false;
        let mut aging = Instant::now();
        let mut current_age: u8 = 0;

        let mut check_for_startup_mode = true;
        let maybe_report = || {
            self.stats
                .report_stats(self.in_mem_only, &self.disk, &self.cache);
        };

        self.stats
            .active_flush_threads
            .fetch_add(1, Ordering::Relaxed);

        loop {
            maybe_report();
            let mut age = None;
            if !exit_when_idle && !self.in_mem_only && aging.elapsed().as_millis() as usize > AGE_MS
            {
                // increment age to get rid of some older things in cache
                current_age = Self::add_age(current_age, 1);
                self.stats.age.store(current_age as u64, Ordering::Relaxed);
                self.current_age.store(current_age, Ordering::Relaxed);
                age = Some(current_age);
                aging = Instant::now();
            }
            if !exit_when_idle && exit.load(Ordering::Relaxed) {
                break;
            }
            if age.is_none() && !found_one {
                let mut m = Measure::start("idle");
                let timeout = self.wait.wait_timeout(Duration::from_millis(200));
                m.stop();
                if !exit_when_idle {
                    self.stats
                        .flushing_idle_us
                        .fetch_add(m.as_us(), Ordering::Relaxed);
                }

                if timeout {
                    if exit_when_idle && exit.load(Ordering::Relaxed) {
                        break;
                    }
                    continue; // otherwise, loop and check for aging and likely wait again
                }
            }
            if !exit_when_idle {
                self.stats.bg_flush_cycles.fetch_add(1, Ordering::Relaxed);
            }
            found_one = false;
            for ix in 0..self.bins {
                let ix = if exit_when_idle {
                    // the other threads just choose random buckets to flush
                    let random: usize = thread_rng().gen();
                    random % self.bins
                } else {
                    ix
                };
                if !exit_when_idle {
                    self.stats.bins_flushed.fetch_add(1, Ordering::Relaxed);
                }
                self.stats.active_flushes.fetch_add(1, Ordering::Relaxed);
                let found_dirty = self.flush(ix, true, age).0;
                self.stats.active_flushes.fetch_sub(1, Ordering::Relaxed);
                if found_dirty {
                    // this bin reported finding something dirty
                    if check_for_startup_mode {
                        if self.startup.load(Ordering::Relaxed) {
                            // if we're still in startup mode, then notify every thread that there are still dirty bins to make sure everyone keeps working
                            self.wait.notify_all();
                        } else {
                            check_for_startup_mode = false;
                        }
                    }
                    maybe_report();
                    found_one = true;
                }
            }
        }

        self.stats
            .active_flush_threads
            .fetch_sub(1, Ordering::Relaxed);
    }
    pub fn new(bucket_map: BucketMapWithEntryType<V>) -> Self {
        let in_mem_only = false;
        let current_age = AtomicU8::new(0);
        let startup = AtomicBool::new(false);
        let bins = bucket_map.num_buckets();
        let cache = (0..bins)
            .map(|_i| RwLock::new(WriteCache::default()))
            .collect();
        let cache_ranges_held = (0..bins).map(|_i| RwLock::new(vec![])).collect();
        let stop_flush = (0..bins).map(|_i| AtomicU64::default()).collect();
        let wait = WaitableCondvar::default();
        let binner = PubkeyBinCalculator16::new(bins);

        let range_start_per_bin = (0..=bins)
            .map(|bin| {
                if bin == bins {
                    Pubkey::new(&[0xff; 32])
                } else {
                    binner.lowest_pubkey_from_bin(bin, bins)
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(bins, bucket_map.num_buckets());

        Self {
            stats: BucketMapHolderStats::default(),
            stop_flush,
            range_start_per_bin,
            current_age,
            cache_ranges_held,
            disk: bucket_map,
            cache,
            bins,
            wait,
            binner,
            startup,
            in_mem_only,
        }
    }
    pub fn set_startup(&self, startup: bool) {
        self.startup.store(startup, Ordering::Relaxed);
    }
    pub fn len_expensive(&self, ix: usize) -> usize {
        if self.in_mem_only {
            self.cache[ix].read().unwrap().len()
        } else {
            let mut keys = HashSet::<Pubkey>::default();
            self.cache[ix].read().unwrap().keys().for_each(|k| {
                keys.insert(*k);
            });
            if let Some(k) = self.disk.keys(ix) {
                k.into_iter().for_each(|k| {
                    keys.insert(k);
                })
            }
            keys.len()
        }
    }

    pub fn flush(&self, ix: usize, bg: bool, age: Option<u8>) -> (bool, usize) {
        if self.in_mem_only {
            return (false, 0);
        }
        let read_lock = self.cache[ix].read().unwrap();
        let mut had_dirty = false;
        if read_lock.is_empty() {
            return (false, 0);
        }
        let startup = self.startup.load(Ordering::Relaxed);

        let (age_comp, do_age, next_age) = age
            .map(|age| (age, true, Self::add_age(age, DEFAULT_AGE_INCREMENT)))
            .unwrap_or((0, false, 0));
        self.stats
            .bucket_flush_calls
            .fetch_add(1, Ordering::Relaxed);
        let len = read_lock.len();
        let mut flush_keys = Vec::with_capacity(len);
        let mut delete_keys = Vec::with_capacity(len);
        let mut flushed = 0;
        let mut get_purges = 0;
        let mut m0 = Measure::start("flush1");
        let mut insert = 0;
        for (k, v) in read_lock.iter() {
            let dirty = v.dirty();
            if dirty && v.insert() {
                insert += 1;
            }
            let mut flush = dirty;
            // could add: || slot_list.len() > 1); // theory is that anything with a len > 1 will be dealt with fairly soon - it is 'hot'
            let keep_this_in_cache = Self::in_cache(&v.slot_list.read().unwrap());
            if bg && keep_this_in_cache {
                // for all account indexes that are in the write cache instead of storage, don't write them to disk yet - they will be updated soon
                flush = false;
            }
            if flush {
                flush_keys.push(*k);
                had_dirty = true;
            }
            let mut delete_key = (do_age && !dirty) || startup;
            let mut purging_non_cached_insert = false;
            if !delete_key && !keep_this_in_cache && v.insert() {
                delete_key = true;
                // get rid of non-cached inserts from the write cache asap - we won't need them again any sooner than any other account
                purging_non_cached_insert = true;
            }
            if delete_key
                && (purging_non_cached_insert
                    || startup
                    || (v.age() == age_comp && !keep_this_in_cache))
            {
                // clear the cache of things that have aged out
                delete_keys.push(*k);
                get_purges += 1;

                // if we should age this key out, then go ahead and flush it
                if !flush && dirty {
                    flush_keys.push(*k);
                }
            }
        }
        m0.stop();
        drop(read_lock);
        if insert > 1_000 {
            // give buckets an idea of what was just inserted
            //self.disk.set_expected_capacity(ix, insert);
            //error!("insert: {}", insert);
        }

        let mut m1 = Measure::start("flush_action");
        {
            let wc = &mut self.cache[ix].write().unwrap(); // maybe get lock for each item?
            for k in flush_keys.into_iter() {
                if let HashMapEntry::Occupied(occupied) = wc.entry(k) {
                    let v = occupied.get();
                    if v.dirty() {
                        if Arc::strong_count(v) > 1 {
                            self.stats
                                .strong_count_no_flush_dirty
                                .fetch_add(1, Ordering::Relaxed);
                            // we have to have a write lock above to know that no client can get the value after this point until we're done flushing
                            // only work on dirty things when there are no outstanding refs to the value
                            continue;
                        }
                        let lookup = v.must_do_lookup_from_disk();
                        self.update_no_cache(
                            &k,
                            |current| {
                                if lookup {
                                    // we have data to insert or update
                                    // we have been unaware if there was anything on disk with this pubkey so far
                                    // so, we may have to merge here
                                    if let Some((slot_list, ref_count)) = current {
                                        let slot_list = slot_list.to_vec();
                                        Self::update_cache_from_disk(
                                            &Arc::new(AccountMapEntryInner {
                                                slot_list: RwLock::new(slot_list),
                                                ref_count: AtomicU64::new(ref_count),
                                                ..AccountMapEntryInner::default()
                                            }),
                                            v,
                                        );
                                    }
                                    // else, we didn't know if there was anything on disk, but there was nothing, so cache is already up to date
                                }
                                // write what is in our cache - it has been merged if necessary
                                Some((v.slot_list.read().unwrap().clone(), v.ref_count()))
                            },
                            None,
                            true,
                        );
                        if lookup {
                            v.set_must_do_lookup_from_disk(false);
                        }
                        let keep_this_in_cache = Self::in_cache(&v.slot_list.read().unwrap());
                        if keep_this_in_cache {
                            v.set_age(next_age); // keep newly updated stuff around
                        }
                        v.set_dirty(false);
                        v.set_insert(false);
                        if self.startup.load(Ordering::Relaxed) {
                            occupied.remove(); // if we're at startup, then we want to get things out of the cache as soon as possible
                        }
                        flushed += 1;
                    }
                }
            }
        }
        m1.stop();

        let mut m2 = Measure::start("flush_delete");
        {
            let ranges = self.cache_ranges_held[ix].read().unwrap();
            let wc = &mut self.cache[ix].write().unwrap(); // maybe get lock for each item?
            for key in delete_keys.into_iter() {
                if ranges
                    .iter()
                    .any(|x| x.as_ref().map(|range| range.contains(&key)).unwrap_or(true))
                {
                    // None means entire bin
                    continue; // skip anything we're not allowed to get rid of
                }
                if let HashMapEntry::Occupied(occupied) = wc.entry(key) {
                    let item = occupied.get();
                    // if someone else dirtied it or aged it newer or someone is holding a refcount to the value, keep it in the cache
                    if item.dirty()
                        || (do_age && (item.age() != age_comp && !startup))
                        || item.must_do_lookup_from_disk()
                    {
                        continue;
                    }

                    if Arc::strong_count(item) > 1 {
                        self.stats
                            .strong_count_no_purge
                            .fetch_add(1, Ordering::Relaxed);
                    }

                    if self.stop_flush(ix) {
                        break;
                    }
                    // otherwise, we were ok to delete the entry from the cache for this key
                    occupied.remove();
                }
            }
        }
        m2.stop();
        let wc = self.cache[ix].read().unwrap(); // maybe get lock for each item?
        let len = wc.len();
        drop(wc);

        self.stats.flush0.fetch_add(m0.as_us(), Ordering::Relaxed);
        self.stats.flush1.fetch_add(m1.as_us(), Ordering::Relaxed);
        self.stats.flush2.fetch_add(m2.as_us(), Ordering::Relaxed);

        if flushed != 0 {
            self.stats
                .cache_flushes
                .fetch_add(flushed as u64, Ordering::Relaxed);
        }
        if get_purges != 0 {
            self.stats
                .get_purges
                .fetch_add(get_purges as u64, Ordering::Relaxed);
        }

        (had_dirty, len)
    }
    pub fn bucket_len(&self, ix: usize) -> u64 {
        if !self.in_mem_only {
            self.disk.bucket_len(ix)
        } else {
            self.cache[ix].read().unwrap().len() as u64
        }
    }
    pub fn bucket_ix(&self, key: &Pubkey) -> usize {
        self.binner.bin_from_pubkey(key)
    }
    pub fn keys<R: RangeBounds<Pubkey>>(
        &self,
        ix: usize,
        range: Option<&R>,
    ) -> Option<Vec<Pubkey>> {
        self.stats.keys.fetch_add(1, Ordering::Relaxed);
        if !self.in_mem_only {
            self.flush(ix, false, None);
            let keys = self.disk.keys(ix);

            let mut wc = self.cache[ix].write().unwrap();
            for key in keys.unwrap_or_default().into_iter() {
                match wc.entry(key) {
                    HashMapEntry::Occupied(occupied) => {
                        // key already exists, so we're fine
                        occupied.get().set_age(self.set_age_to_future());
                        assert!(!occupied.get().confirmed_not_on_disk());
                    }
                    HashMapEntry::Vacant(vacant) => {
                        let must_do_lookup_from_disk = true;
                        vacant.insert(self.allocate(
                            SlotList::default(),
                            RefCount::MAX,
                            false,
                            false,
                            must_do_lookup_from_disk,
                            false,
                        ));
                    }
                }
            }
        }

        // keys come entirely from in-mem cache
        let wc = self.cache[ix].read().unwrap();
        let mut result = Vec::with_capacity(wc.len());
        for i in wc.keys() {
            if range.map(|range| range.contains(i)).unwrap_or(true) {
                result.push(*i);
            }
        }
        Some(result)
    }

    pub fn num_buckets(&self) -> usize {
        self.bins
    }

    fn upsert_item_in_cache(
        &self,
        entry: &WriteCacheEntryArc<V>,
        new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        previous_slot_entry_was_cached: bool,
        mut m1: Measure,
    ) {
        let instance = entry;

        if instance.must_do_lookup_from_disk() {
            // this item exists in the cache, but we don't know if it exists on disk or not
            assert!(!instance.confirmed_not_on_disk());
            // we can call update on the item as it is. same slot will result in reclaims, different slot will result in slot list growing
            // must_do_lookup_from_disk should remain
        }

        instance.set_age(self.set_age_to_future());
        instance.set_dirty(true);
        if instance.confirmed_not_on_disk() {
            self.stats.using_empty_get.fetch_add(1, Ordering::Relaxed);
            instance.ref_count.store(0, Ordering::Relaxed);
            assert!(instance.slot_list.read().unwrap().is_empty());
            instance.set_confirmed_not_on_disk(false); // we are inserted now if we were 'confirmed_not_on_disk' before. update_static below handles this fine
        }
        self.stats.updates_in_cache.fetch_add(1, Ordering::Relaxed);

        let (slot, new_entry) = new_value.slot_list.write().unwrap().remove(0);
        let addref = WriteAccountMapEntry::update_slot_list(
            &mut instance.slot_list.write().unwrap(),
            slot,
            new_entry,
            reclaims,
            previous_slot_entry_was_cached,
        );
        if addref {
            instance.add_un_ref(true);
        }
        m1.stop();
        self.stats
            .update_cache_us
            .fetch_add(m1.as_ns(), Ordering::Relaxed);
        self.wait.notify_all();
    }

    // If the slot list for pubkey exists in the index and is empty, remove the index entry for pubkey and return true.
    // Return false otherwise.
    pub fn remove_if_slot_list_empty(&self, index: usize, key: &Pubkey) -> bool {
        // this causes the item to be updated from disk - we need the full slot list here
        let get_result = self.get(index, key); // hold this open so we have a ref to the arc
        match &get_result {
            Some(item) => {
                if !item.slot_list.read().unwrap().is_empty() {
                    // not empty slot list
                    return false;
                }
            }
            None => {
                return false; // not in accounts index at all - should this be a true?
            }
        }
        assert!(Arc::strong_count(get_result.as_ref().unwrap()) > 1);

        // if we made it here, the item is fully loaded in the cache
        let mut w_index = self.cache[index].write().unwrap();
        match w_index.entry(*key) {
            Entry::Occupied(index_entry) => {
                assert!(!index_entry.get().must_do_lookup_from_disk());
                if index_entry.get().slot_list.read().unwrap().is_empty() {
                    index_entry.remove();
                    if !self.in_mem_only {
                        self.disk.delete_key(key);
                    }
                    return true;
                }
            }
            Entry::Vacant(_) => {
                panic!("item was purged from cache - should not be possible since we have a refcount to the arc");
            }
        }
        false
    }

    pub fn upsert(
        &self,
        key: &Pubkey,
        new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        previous_slot_entry_was_cached: bool,
    ) {
        let ix = self.bucket_ix(key);
        if TRY_READ_LOCKS_FIRST {
            // maybe to eliminate race conditions, we have to have a write lock?
            // try read lock first
            {
                let m1 = Measure::start("upsert");
                let wc = &mut self.cache[ix].read().unwrap();
                let res = wc.get(key);
                if let Some(occupied) = res {
                    // already in cache, so call update_static function
                    // this may only be technically necessary if !previous_slot_entry_was_cached, where we have to return reclaims
                    self.get_caching(occupied, key); // updates from disk if necessary
                    self.upsert_item_in_cache(
                        occupied,
                        new_value,
                        reclaims,
                        previous_slot_entry_was_cached,
                        m1,
                    );
                    return;
                }
            }
        }

        // try write lock
        let mut m1 = Measure::start("upsert_write");
        let wc = &mut self.cache[ix].write().unwrap();
        let res = wc.entry(*key);
        match res {
            HashMapEntry::Occupied(occupied) => {
                let item = occupied.get();
                self.get_caching(item, key); // updates from disk if necessary
                                             // already in cache, so call update_static function
                self.upsert_item_in_cache(
                    item,
                    new_value,
                    reclaims,
                    previous_slot_entry_was_cached,
                    m1,
                );
            }
            HashMapEntry::Vacant(vacant) => {
                /*
                if previous_slot_entry_was_cached && false {
                    // todo
                    // we don't have to go to disk to look this thing up yet
                    // not on disk, not in cache, so add to cache
                    /*
                    let must_do_lookup_from_disk = true;
                    self.inserts_without_checking_disk
                        .fetch_add(1, Ordering::Relaxed);
                    vacant.insert(self.allocate(
                        &new_value.slot_list,
                        new_value.ref_count,
                        true,
                        true,
                        must_do_lookup_from_disk,
                        confirmed_not_on_disk,
                    ));
                    */
                    return; // we can be satisfied that this index will be looked up later
                }*/
                let r = self.get_no_cache(key); // maybe move this outside lock - but race conditions unclear
                if let Some(current) = r {
                    // not in cache, on disk
                    let (slot, new_entry) = new_value.slot_list.write().unwrap().remove(0);
                    let mut slot_list = current.slot_list.write().unwrap();
                    let addref = WriteAccountMapEntry::update_slot_list(
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
                    drop(slot_list);
                    vacant.insert(current);
                    m1.stop();
                    self.stats
                        .update_cache_us
                        .fetch_add(m1.as_ns(), Ordering::Relaxed);
                } else {
                    // not on disk, not in cache, so add to cache
                    self.stats.inserts.fetch_add(1, Ordering::Relaxed);
                    new_value.set_dirty(true);
                    new_value.set_insert(true);
                    vacant.insert(new_value);
                }
                self.wait.notify_all();
            }
        }
    }

    fn in_cache(slot_list: SlotSlice<V>) -> bool {
        slot_list.iter().any(|(_slot, info)| info.is_cached())
    }

    fn update_cache_from_disk<'a>(
        disk_entry: &'a AccountMapEntry<V>,
        cache_entry_to_update: &'a WriteCacheEntry<V>,
    ) {
        let previous_slot_entry_was_cached = false;
        let mut _reclaims = Vec::default();
        let mut slot_list_cache = cache_entry_to_update.slot_list.write().unwrap();
        let mut slot_list_disk = disk_entry.slot_list.write().unwrap();
        for (slot, new_entry) in slot_list_cache.iter() {
            let addref = WriteAccountMapEntry::update_slot_list(
                &mut slot_list_disk,
                *slot,
                *new_entry,
                &mut _reclaims,
                previous_slot_entry_was_cached,
            );
            if addref {
                disk_entry.add_un_ref(true);
            }
        }
        // now that we're done, disk_entry has the correct slot_list and ref_count
        cache_entry_to_update.set_ref_count(disk_entry.ref_count());
        // seems like this could be faster, but swap didn't work like I wanted...
        slot_list_cache.clear();
        slot_list_cache.append(&mut slot_list_disk);
        cache_entry_to_update.set_dirty(true);
        // race conditions here - if someone else already has a ref to the arc, it is difficult to make the refcounts work out right
        // but this is internal only - no 'get' should have returned the account data prior to us reconciling with disk
    }

    pub fn update_no_cache<F>(
        &self,
        key: &Pubkey,
        updatefn: F,
        current_value: Option<&V2<V>>,
        force_not_insert: bool,
    ) where
        F: Fn(Option<(&[SlotT<V>], u64)>) -> Option<(Vec<SlotT<V>>, u64)>,
    {
        let must_do_lookup_from_disk = false;
        let confirmed_not_on_disk = false;

        if current_value.is_none() && !force_not_insert {
            if true {
                if !self.in_mem_only {
                    // send straight to disk. if we try to keep it in the write cache, then 'keys' will be incorrect
                    self.disk.update(key, updatefn);
                }
            } else {
                let result = updatefn(None);
                if let Some(result) = result {
                    // stick this in the write cache and flush it later
                    let ix = self.bucket_ix(key);
                    let wc = &mut self.cache[ix].write().unwrap();
                    wc.insert(
                        *key,
                        self.allocate(
                            result.0,
                            result.1,
                            true,
                            true,
                            must_do_lookup_from_disk,
                            confirmed_not_on_disk,
                        ),
                    );
                } else {
                    panic!("should have returned a value from updatefn");
                }
            }
        } else if !self.in_mem_only {
            // update
            self.disk.update(key, updatefn);
        }
    }

    fn allocate(
        &self,
        slot_list: SlotList<V>,
        ref_count: RefCount,
        dirty: bool,
        insert: bool,
        must_do_lookup_from_disk: bool,
        confirmed_not_on_disk: bool,
    ) -> WriteCacheEntryArc<V> {
        assert!(!(insert && !dirty));
        Arc::new(AccountMapEntryInner {
            slot_list: RwLock::new(slot_list),
            ref_count: AtomicU64::new(ref_count),
            dirty: AtomicBool::new(dirty),
            age: AtomicU8::new(self.set_age_to_future()),
            insert: AtomicBool::new(insert),
            must_do_lookup_from_disk: AtomicBool::new(must_do_lookup_from_disk),
            confirmed_not_on_disk: AtomicBool::new(confirmed_not_on_disk),
        })
    }

    fn upsert_in_cache<F>(&self, key: &Pubkey, updatefn: F, current_value: Option<&V2<V>>)
    where
        F: Fn(Option<&V2<V>>) -> Option<(Vec<SlotT<V>>, u64)>,
    {
        let must_do_lookup_from_disk = false;
        let confirmed_not_on_disk = false;
        // we are an update
        let result = updatefn(current_value);
        if let Some(result) = result {
            // stick this in the write cache and flush it later
            let ix = self.bucket_ix(key);
            let wc = &mut self.cache[ix].write().unwrap();

            match wc.entry(*key) {
                HashMapEntry::Occupied(mut occupied) => {
                    let gm = occupied.get_mut();
                    let insert = gm.insert();
                    *gm = self.allocate(
                        result.0,
                        result.1,
                        true,
                        insert,
                        must_do_lookup_from_disk,
                        confirmed_not_on_disk,
                    );
                    assert_eq!(occupied.get().insert(), insert);
                    assert_eq!(
                        occupied.get().confirmed_not_on_disk(),
                        confirmed_not_on_disk
                    );
                }
                HashMapEntry::Vacant(vacant) => {
                    vacant.insert(self.allocate(
                        result.0,
                        result.1,
                        true,
                        true,
                        must_do_lookup_from_disk,
                        confirmed_not_on_disk,
                    ));
                    self.wait.notify_all(); // we have put something in the write cache that needs to be flushed sometime - should there be more of these calls each time?
                }
            }
        } else {
            panic!("expected value");
        }
    }

    pub fn update<F>(&self, key: &Pubkey, updatefn: F, current_value: Option<&V2<V>>)
    where
        F: Fn(Option<&V2<V>>) -> Option<(Vec<SlotT<V>>, u64)>,
    {
        if current_value.is_none() {
            // we are an insert
            self.stats.inserts.fetch_add(1, Ordering::Relaxed);
        } else {
            if VERIFY_GET_ON_INSERT {
                let ix = self.bucket_ix(key);
                assert!(self.get(ix, key).is_some());
            }
            // if we have a previous value, then that item is currently open and locked, so it could not have been changed. Thus, this is an in-cache update as long as we are caching gets.
            self.stats.updates_in_cache.fetch_add(1, Ordering::Relaxed);
        }
        self.upsert_in_cache(key, updatefn, current_value);
    }

    fn disk_to_cache_entry(ref_count: RefCount, slot_list: SlotList<V>) -> V2<V> {
        Arc::new(AccountMapEntryInner {
            ref_count: AtomicU64::new(ref_count),
            slot_list: RwLock::new(slot_list),
            ..AccountMapEntryInner::default()
        })
    }

    pub fn get_no_cache(&self, key: &Pubkey) -> Option<V2<V>> {
        let mut m1 = Measure::start("get_no_cache");
        if self.in_mem_only {
            return None;
        }
        let r = self.disk.get(key);
        let r = r.map(|(ref_count, slot_list)| Self::disk_to_cache_entry(ref_count, slot_list));
        m1.stop();
        self.stats
            .get_disk_us
            .fetch_add(m1.as_ns(), Ordering::Relaxed);
        if r.is_some() {
            self.stats.gets_from_disk.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats
                .gets_from_disk_empty
                .fetch_add(1, Ordering::Relaxed);
        }
        r
    }

    fn set_age_to_future(&self) -> u8 {
        let add_age = if self.startup.load(Ordering::Relaxed) {
            1
        } else {
            DEFAULT_AGE_INCREMENT
        };
        let current_age = self.current_age.load(Ordering::Relaxed);
        Self::add_age(current_age, add_age)
    }

    fn maybe_merge_disk_into_cache<F>(
        &self,
        item_in_cache: &WriteCacheEntryArc<V>,
        get_item_on_disk: F,
    ) where
        F: FnOnce() -> Option<V2<V>>,
    {
        if item_in_cache.must_do_lookup_from_disk() {
            // we have inserted or updated this item, but we have never reconciled with the disk, so we have to do that now since a caller is requesting the complete item
            if let Some(disk_v) = get_item_on_disk() {
                assert!(!self.in_mem_only);
                // we now found the item on disk, so we need to update the disk state with cache changes, then we need to update the cache to be the new dirty state
                Self::update_cache_from_disk(&disk_v, item_in_cache);
            }
            // else, we now know the cache entry was the only info that exists for this account, so we have all we need already
            item_in_cache.set_must_do_lookup_from_disk(false);
            //item_in_cache.set_confirmed_not_on_disk(true); not sure about this - items we inserted that are not on disk are ok
            assert!(!item_in_cache.slot_list.read().unwrap().is_empty()); // get rid of eventually
        }
    }

    fn get_caching<'b>(
        &self,
        item_in_cache: &'b WriteCacheEntryArc<V>,
        key: &Pubkey,
    ) -> Option<()> {
        item_in_cache.set_age(self.set_age_to_future());
        self.stats.gets_from_cache.fetch_add(1, Ordering::Relaxed);
        if item_in_cache.confirmed_not_on_disk() {
            return None; // does not really exist
        }
        self.maybe_merge_disk_into_cache(item_in_cache, || self.get_no_cache(key));

        Some(())
    }

    fn put_disk_items_in_cache<'a>(
        &'a self,
        disk_data: Vec<BucketMapKeyValue<SlotT<V>>>,
        lock: &mut CacheWriteLock<'a, V>, // later, use a read lock as long as possible
    ) {
        disk_data.into_iter().for_each(
            |BucketMapKeyValue {
                 pubkey,
                 ref_count,
                 slot_list,
             }| {
                let get_item_on_disk = || Some(Self::disk_to_cache_entry(ref_count, slot_list));
                match lock.entry(pubkey) {
                    HashMapEntry::Occupied(occupied) => {
                        // make sure cache is up to date
                        let v = occupied.get();
                        assert!(!v.confirmed_not_on_disk());
                        self.maybe_merge_disk_into_cache(v, get_item_on_disk);
                        v.set_age(self.set_age_to_future());
                    }
                    HashMapEntry::Vacant(vacant) => {
                        vacant.insert(get_item_on_disk().unwrap());
                    }
                }
            },
        )
    }

    fn stop_flush(&self, ix: usize) -> bool {
        self.stop_flush[ix].load(Ordering::Relaxed) > 0
    }

    fn put_range_in_cache<R>(&self, ix: usize, range: Option<&R>)
    where
        R: RangeBounds<Pubkey>,
    {
        assert!(self.stop_flush(ix)); // caller should be controlling the lifetime of how long this needs to be present
        let mut m = Measure::start("range");
        if !self.in_mem_only {
            // figure out if we need to load from disk or if cache is up to date for our range
            let mut load = true;

            let caller_wants_entire_bucket = range.is_none();
            for held_range in self.cache_ranges_held[ix].read().unwrap().iter() {
                if held_range.is_none() {
                    // there is a 'whole' range already held
                    // since we incremented stop_flush before we checked and the range is only added to this list:
                    //  1. while another stop_flush is incremented
                    //  2. and after the range's contents were put into the cache
                    // Then since we beat the range being removed from cache_ranges_held
                    //  we can know that the range still exists because of our stop_flush increment
                    load = false;
                    break;
                }

                if caller_wants_entire_bucket {
                    // otherwise, caller wants entire range but this range is partial
                    // comparing overlapping partial ranges held in memory is not currently implemented.
                    // That isn't a use case that seems important.
                } else {
                    // caller wants part of a bucket's range
                    // part of the bucket's range is being held
                    // see if this held range contains the entire range the caller wants
                    let range_ref = range.as_ref().unwrap();
                    let held_range = held_range.as_ref().unwrap();
                    if self.range_contains_bound(ix, true, &range_ref.start_bound(), held_range)
                        && self.range_contains_bound(ix, false, &range_ref.end_bound(), held_range)
                    {
                        load = false;
                        break;
                    }
                }
            }

            if load {
                if let Some(disk_range) = self.disk.range(ix, range) {
                    let mut lock = self.cache[ix].write().unwrap();
                    self.put_disk_items_in_cache(disk_range, &mut lock);
                }
            }
        }
        m.stop();
        self.stats.range_us.fetch_add(m.as_us(), Ordering::Relaxed);
    }

    fn range_contains_bound(
        &self,
        ix: usize,
        is_lower: bool,
        bounds: &Bound<&Pubkey>,
        range: &RangeInclusive<Pubkey>,
    ) -> bool {
        let compare = if is_lower {
            match range.start_bound() {
                Bound::Included(bound) => bound,
                _ => panic!("unexpected"),
            }
        } else {
            match range.end_bound() {
                Bound::Included(bound) => bound,
                _ => panic!("unexpected"),
            }
        };
        // messy around highest item of bin - normally we expect to not hit a bin boundary exactly
        // start bound is >= range held.start
        // end bound is <= range held.end
        match bounds {
            Bound::Included(bound) | Bound::Excluded(bound) => {
                if is_lower {
                    bound >= &compare
                } else {
                    bound < &compare
                }
            }
            Bound::Unbounded => {
                if is_lower {
                    // caller wants starting at 0, so make sure we are holding to the beginning of this bucket
                    compare <= &self.range_start_per_bin[ix]
                } else {
                    // caller wants ending at last pubkey value, so make sure we are holding to the end of this bucket
                    compare >= &self.range_start_per_bin[ix + 1]
                }
            }
        }
    }

    pub fn range<R>(&self, ix: usize, range: Option<&R>) -> Vec<(Pubkey, AccountMapEntry<V>)>
    where
        R: RangeBounds<Pubkey>,
    {
        // this causes the items we load into the cache to remain there and not be flushed.
        // So, we can know all items on disk are present in the cache.
        self.start_stop_flush(ix, true);

        self.put_range_in_cache(ix, range);
        let lock = self.cache[ix].read().unwrap();
        let mut result = Vec::with_capacity(lock.len());
        for (k, v) in lock.iter() {
            if range.map(|range| range.contains(k)).unwrap_or(true) {
                result.push((*k, v.clone()));
            }
        }
        self.start_stop_flush(ix, false);
        result
    }

    pub fn get(&self, ix: usize, key: &Pubkey) -> Option<V2<V>> {
        let must_do_lookup_from_disk = false;
        if TRY_READ_LOCKS_FIRST {
            let mut m1 = Measure::start("get");
            let wc = &mut self.cache[ix].read().unwrap();
            let item_in_cache = wc.get(key);
            m1.stop();
            if let Some(item_in_cache) = item_in_cache {
                if item_in_cache.confirmed_not_on_disk() {
                    self.stats
                        .get_cache_us
                        .fetch_add(m1.as_ns(), Ordering::Relaxed);
                    return None; // does not really exist
                }

                // we can't update the cache with only a read lock
                if !item_in_cache.must_do_lookup_from_disk() {
                    self.stats
                        .get_cache_us
                        .fetch_add(m1.as_ns(), Ordering::Relaxed);
                    return Some(item_in_cache.clone());
                }
            }
        }
        // get caching
        {
            let mut m1 = Measure::start("get2");
            let wc = &mut self.cache[ix].write().unwrap();
            let res = wc.entry(*key);
            match res {
                HashMapEntry::Occupied(occupied) => {
                    let res = occupied.get();
                    self.stats
                        .get_cache_us
                        .fetch_add(m1.as_ns(), Ordering::Relaxed);
                    self.get_caching(res, key).map(|_| res.clone())
                }
                HashMapEntry::Vacant(vacant) => {
                    let r = self.get_no_cache(key);
                    let r: Option<&AccountMapEntry<V>> = Some(if let Some(loaded) = r {
                        vacant.insert(loaded)
                    } else {
                        // we looked this up. it does not exist. let's insert a marker in the cache saying it doesn't exist. otherwise, we have to look it up again on insert!
                        let confirmed_not_on_disk = true;
                        vacant.insert(self.allocate(
                            SlotList::default(),
                            RefCount::MAX,
                            false,
                            false,
                            must_do_lookup_from_disk,
                            confirmed_not_on_disk,
                        ));
                        return None; // not on disk, not in cache, so get should return None
                    });
                    m1.stop();
                    self.stats
                        .update_cache_us
                        .fetch_add(m1.as_ns(), Ordering::Relaxed);
                    r.cloned()
                }
            }
        }
    }
    pub fn delete_key(&self, ix: usize, key: &Pubkey) {
        self.stats.deletes.fetch_add(1, Ordering::Relaxed);
        {
            let wc = &mut self.cache[ix].write().unwrap();
            wc.remove(key);
        }
        if !self.in_mem_only {
            self.disk.delete_key(key)
        }
    }
}
