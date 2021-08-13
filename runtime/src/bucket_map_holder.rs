use crate::accounts_index::AccountMapEntry;
use crate::accounts_index::{IsCached, RefCount, SlotList, WriteAccountMapEntry};
use crate::hybrid_btree_map::{SlotT, V2};
use crate::pubkey_bins::PubkeyBinCalculator16;
use solana_bucket_map::bucket_map::BucketMap;
use solana_measure::measure::Measure;
use solana_sdk::pubkey::Pubkey;
use std::fmt::Debug;
use std::ops::RangeBounds;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::{RwLock, RwLockWriteGuard};
use std::time::Instant;

use std::collections::{hash_map::Entry as HashMapEntry, HashMap};
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

pub const DEFAULT_AGE: u8 = 25;
pub const VERIFY_GET_ON_INSERT: bool = false;
pub const UPDATE_CACHING: bool = true;
pub const INSERT_CACHING: bool = true;
pub const GET_CACHING: bool = true;

impl Hasher for MyHasher {
    fn write(&mut self, bytes: &[u8]) {
        let len = bytes.len();
        let start = len - 8;
        self.hash ^= u64::from_be_bytes(bytes[start..].try_into().unwrap());
        //error!("hash bytes: {}", bytes.len());
        //assert_eq!(self.hash, 0);
        //self.hash = u64::from_be_bytes(bytes[24..32].try_into().unwrap());
        /*
        for i in 0..bytes.len() {
            self.hash = (self.hash << 1) ^ (bytes[i] as u64);
        }
        */
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}
/*
pub struct myhash {

}
impl BuildHasher for myhash {
    type Hasher = MyHasher;
    fn build_hasher(&self) -> Self::Hasher
    {
        MyHasher::default()
    }
}*/
type MyBuildHasher = BuildHasherDefault<MyHasher>;

#[derive(Debug)]
pub struct WriteCacheEntry<V> {
    pub data: V2<V>,
    pub age: u8,
    pub dirty: bool,
    pub insert: bool,
    pub must_do_lookup_from_disk: bool,
    pub confirmed_not_on_disk: bool,
}

#[derive(Debug)]
pub struct WriteCacheEntryArc<V> {
    pub instance: RwLock<WriteCacheEntry<V>>, //Arc<WriteCacheEntry<V>>,
}

#[derive(Debug)]
pub struct BucketMapWriteHolder<V> {
    pub current_age: AtomicU8,
    pub disk: BucketMap<SlotT<V>>,
    pub write_cache: Vec<RwLock<WriteCache<WriteCacheEntryArc<V>>>>,
    pub get_disk_us: AtomicU64,
    pub update_dist_us: AtomicU64,
    pub get_cache_us: AtomicU64,
    pub update_cache_us: AtomicU64,
    pub write_cache_flushes: AtomicU64,
    pub bucket_flush_calls: AtomicU64,
    pub startup: AtomicBool,
    pub get_purges: AtomicU64,
    pub gets_from_disk: AtomicU64,
    pub gets_from_disk_empty: AtomicU64,
    pub gets_from_cache: AtomicU64,
    pub updates: AtomicU64,
    pub flush0: AtomicU64,
    pub flush1: AtomicU64,
    pub flush2: AtomicU64,
    pub flush3: AtomicU64,
    pub using_empty_get: AtomicU64,
    pub insert_without_lookup: AtomicU64,
    pub updates_in_cache: AtomicU64,
    pub addrefs: AtomicU64,
    pub unrefs: AtomicU64,
    pub range: AtomicU64,
    pub range_us: AtomicU64,
    pub keys: AtomicU64,
    pub inserts: AtomicU64,
    pub inserts_without_checking_disk: AtomicU64,
    pub deletes: AtomicU64,
    pub bins: usize,
    pub wait: WaitableCondvar,
    binner: PubkeyBinCalculator16,
    pub unified_backing: bool,
    pub in_mem_only: bool,
}

impl<V: IsCached> BucketMapWriteHolder<V> {
    pub fn update_or_insert_async(&self, ix: usize, key: Pubkey, new_value: AccountMapEntry<V>) {
        // if in cache, update now in cache
        // if known not present, insert into cache
        // if not in cache, put in cache, with flag saying we need to update later
        let wc = &mut self.write_cache[ix].write().unwrap();
        let m1 = Measure::start("");
        let res = wc.entry(key);
        match res {
            HashMapEntry::Occupied(occupied) => {
                // already in cache, so call update_static function
                let reclaims_must_be_empty = true;
                let mut reclaims = Vec::default();
                self.upsert_item_in_cache(
                    occupied.get(),
                    new_value,
                    &mut reclaims,
                    reclaims_must_be_empty,
                    m1,
                );
                return;
            }
            HashMapEntry::Vacant(vacant) => {
                let must_do_lookup_from_disk = true;
                let confirmed_not_on_disk = false;
                // not in cache - may or may not be on disk
                vacant.insert(self.allocate(
                    &new_value.slot_list,
                    new_value.ref_count,
                    true,
                    true,
                    must_do_lookup_from_disk,
                    confirmed_not_on_disk,
                ));
            }
        }
    }

    fn add_age(age: u8, inc: u8) -> u8 {
        age.wrapping_add(inc)
    }

    pub fn dump_metrics(&self) {
        self.distribution2();
    }

    pub fn bg_flusher(&self, exit: Arc<AtomicBool>) {
        let mut found_one = false;
        let mut last = Instant::now();
        let mut aging = Instant::now();
        let mut current_age: u8 = 0;
        loop {
            if last.elapsed().as_millis() > 1000 {
                self.distribution2();
                last = Instant::now();
            }
            let mut age = None;
            if !self.in_mem_only && aging.elapsed().as_millis() > 400 {
                // time of 1 slot
                current_age = Self::add_age(current_age, 1); // % DEFAULT_AGE; // no reason to pass by something too often if we accidentally miss it...
                self.current_age.store(current_age, Ordering::Relaxed);
                age = Some(current_age);
                aging = Instant::now();
            }
            if exit.load(Ordering::Relaxed) {
                break;
            }
            if age.is_none() && !found_one && self.wait.wait_timeout(Duration::from_millis(500)) {
                continue;
            }
            found_one = false;
            for ix in 0..self.bins {
                if self.flush(ix, true, age).0 {
                    found_one = true;
                }
            }
        }
    }
    pub fn new(bucket_map: BucketMap<SlotT<V>>) -> Self {
        let in_mem_only = false;
        let current_age = AtomicU8::new(0);
        let get_disk_us = AtomicU64::new(0);
        let update_dist_us = AtomicU64::new(0);
        let get_cache_us = AtomicU64::new(0);
        let update_cache_us = AtomicU64::new(0);

        let addrefs = AtomicU64::new(0);
        let unrefs = AtomicU64::new(0);
        let range = AtomicU64::new(0);
        let range_us = AtomicU64::new(0);
        let keys = AtomicU64::new(0);
        let write_cache_flushes = AtomicU64::new(0);
        let bucket_flush_calls = AtomicU64::new(0);
        let get_purges = AtomicU64::new(0);
        let startup = AtomicBool::new(false);
        let gets_from_disk = AtomicU64::new(0);
        let gets_from_disk_empty = AtomicU64::new(0);
        let using_empty_get = AtomicU64::new(0);
        let insert_without_lookup = AtomicU64::new(0);
        let gets_from_cache = AtomicU64::new(0);
        let updates_in_cache = AtomicU64::new(0);
        let inserts_without_checking_disk = AtomicU64::new(0);
        let updates = AtomicU64::new(0);
        let flush0 = AtomicU64::new(0);
        let flush1 = AtomicU64::new(0);
        let flush2 = AtomicU64::new(0);
        let flush3 = AtomicU64::new(0);

        let inserts = AtomicU64::new(0);
        let deletes = AtomicU64::new(0);
        let bins = bucket_map.num_buckets();
        let write_cache = (0..bins)
            .map(|_i| RwLock::new(WriteCache::default()))
            .collect::<Vec<_>>();
        let wait = WaitableCondvar::default();
        let binner = PubkeyBinCalculator16::new(bins);
        let unified_backing = false;
        assert_eq!(bins, bucket_map.num_buckets());

        Self {
            current_age,
            disk: bucket_map,
            write_cache,
            bins,
            wait,
            using_empty_get,
            insert_without_lookup,
            gets_from_cache,
            updates_in_cache,
            inserts_without_checking_disk,
            gets_from_disk,
            gets_from_disk_empty,
            deletes,
            write_cache_flushes,
            updates,
            inserts,
            flush0,
            flush1,
            flush2,
            flush3,
            binner,
            unified_backing,
            get_purges,
            bucket_flush_calls,
            startup,
            get_disk_us,
            update_dist_us,
            get_cache_us,
            update_cache_us,
            addrefs,
            unrefs,
            range,
            range_us,
            keys,
            in_mem_only,
        }
    }
    pub fn set_startup(&self, startup: bool) {
        self.startup.store(startup, Ordering::Relaxed);
    }
    pub fn flush(&self, ix: usize, bg: bool, age: Option<u8>) -> (bool, usize) {
        if self.in_mem_only {
            return (false, 0);
        }
        //error!("start flusha: {}", ix);
        let read_lock = self.write_cache[ix].read().unwrap();
        //error!("start flusha: {}", ix);
        let mut had_dirty = false;
        if read_lock.is_empty() {
            return (false, 0);
        }
        let startup = self.startup.load(Ordering::Relaxed);

        //error!("start flush: {}", ix);
        let (age_comp, do_age, next_age) = age
            .map(|age| (age, true, Self::add_age(age, DEFAULT_AGE)))
            .unwrap_or((0, false, 0));
        self.bucket_flush_calls.fetch_add(1, Ordering::Relaxed);
        let len = read_lock.len();
        let mut flush_keys = Vec::with_capacity(len);
        let mut delete_keys = Vec::with_capacity(len);
        let mut flushed = 0;
        let mut get_purges = 0;
        let mut m0 = Measure::start("");
        for (k, v) in read_lock.iter() {
            let instance = v.instance.read().unwrap();
            let dirty = instance.dirty;
            let mut flush = dirty;
            let keep_this_in_cache = Self::in_cache(&instance.data.slot_list); // || instance.data.slot_list.len() > 1);
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
            if !delete_key && !keep_this_in_cache && instance.insert {
                delete_key = true;
                // get rid of non-cached inserts from the write cache asap - we won't need them again any sooner than any other account
                purging_non_cached_insert = true;
            }
            if delete_key {
                if purging_non_cached_insert
                    || startup
                    || (instance.age == age_comp && !keep_this_in_cache)
                {
                    // clear the cache of things that have aged out
                    delete_keys.push(*k);
                    get_purges += 1;

                    // if we should age this key out, then go ahead and flush it
                    if !flush && dirty {
                        flush_keys.push(*k);
                    }
                } else {
                    //error!("not delete_key2: {:?}, {}", instance, k);
                }
            } else {
                //error!("not delete_key: {:?}", instance);
            }
        }
        m0.stop();

        let mut m3 = Measure::start("");
        /*
        // time how long just to get everything from disk
        for (k, mut v) in read_lock.iter() {
            self.disk.get(k);
        }
        */
        drop(read_lock);
        m3.stop();

        let mut m1 = Measure::start("");
        {
            let wc = &mut self.write_cache[ix].write().unwrap(); // maybe get lock for each item?
            for k in flush_keys.into_iter() {
                match wc.entry(k) {
                    HashMapEntry::Occupied(occupied) => {
                        let mut instance = occupied.get().instance.write().unwrap();
                        if instance.dirty {
                            let merged_value = RwLock::new(None);
                            let lookup = instance.must_do_lookup_from_disk;
                            self.update_no_cache(
                                &k,
                                |current| {
                                    if lookup {
                                        // we have data to insert or update
                                        // we have been unaware if there was anything on disk with this pubkey so far
                                        // so, we may have to merge here
                                        if let Some((slot_list, mut ref_count)) = current {
                                            let mut slot_list = slot_list.to_vec();
                                            Self::merge_slot_lists(
                                                &mut slot_list,
                                                &mut ref_count,
                                                &instance,
                                            );
                                            *merged_value.write().unwrap() =
                                                Some((slot_list.clone(), ref_count));
                                            return Some((slot_list, ref_count));
                                        }
                                        // else, we didn't know if there was anything on disk, but there was nothing, so we are done
                                    }
                                    // write what is in our cache - it has been merged if necessary
                                    Some((instance.data.slot_list.clone(), instance.data.ref_count))
                                },
                                None,
                                true,
                            );
                            if lookup {
                                // we did a lookup, so put the merged value back into the cache
                                if let Some((slot_list, ref_count)) =
                                    merged_value.into_inner().unwrap()
                                {
                                    instance.data.slot_list = slot_list;
                                    instance.data.ref_count = ref_count;
                                }
                                instance.must_do_lookup_from_disk = false;
                            }
                            let mut keep_this_in_cache = true;
                            if instance.insert {
                                keep_this_in_cache = Self::in_cache(&instance.data.slot_list);
                                // || instance.data.slot_list.len() > 1);
                            }
                            if keep_this_in_cache {
                                instance.age = next_age; // keep newly updated stuff around
                            }
                            instance.dirty = false;
                            instance.insert = false;
                            if !keep_this_in_cache {
                                // delete immediately to avoid a race condition
                                drop(instance);
                                occupied.remove();
                            }
                            flushed += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        m1.stop();

        let mut m2 = Measure::start("");
        {
            let wc = &mut self.write_cache[ix].write().unwrap(); // maybe get lock for each item?
            for k in delete_keys.iter() {
                if let Some(item) = wc.remove(k) {
                    //error!("mid flush: {}, {}", ix, line!());
                    let instance = item.instance.write().unwrap();
                    //error!("mid flush: {}, {}", ix, line!());
                    // if someone else dirtied it or aged it newer, so re-insert it into the cache
                    if instance.dirty
                        || (do_age && instance.age != age_comp)
                        || instance.must_do_lookup_from_disk
                    {
                        // todo panic!("re-adding: {}, {:?}, age: {}", k, instance, do_age);
                        drop(instance);
                        wc.insert(*k, item);
                    }
                    // otherwise, we were ok to delete that key from the cache
                }
            }
        }
        m2.stop();
        let wc = &mut self.write_cache[ix].read().unwrap(); // maybe get lock for each item?
        let len = wc.len();
        drop(wc);

        self.flush0.fetch_add(m0.as_us(), Ordering::Relaxed);
        self.flush1.fetch_add(m1.as_us(), Ordering::Relaxed);
        self.flush2.fetch_add(m2.as_us(), Ordering::Relaxed);
        self.flush3.fetch_add(m3.as_us(), Ordering::Relaxed);

        if flushed != 0 {
            self.write_cache_flushes
                .fetch_add(flushed as u64, Ordering::Relaxed);
        }
        if get_purges != 0 {
            self.get_purges
                .fetch_add(get_purges as u64, Ordering::Relaxed);
        }
        //error!("end flush: {}", ix);

        (had_dirty, len)
    }
    pub fn bucket_len(&self, ix: usize) -> u64 {
        if !self.in_mem_only {
            self.disk.bucket_len(ix)
        } else {
            self.write_cache[ix].read().unwrap().len() as u64
        }
    }
    pub fn bucket_ix(&self, key: &Pubkey) -> usize {
        let b = self.binner.bin_from_pubkey(key);
        /*
        assert_eq!(
            b,
            self.disk.bucket_ix(key),
            "be {}",
            Self::read_be_u64(key.as_ref()),
        );
        */
        b
    }
    pub fn keys3<R: RangeBounds<Pubkey>>(
        &self,
        ix: usize,
        _range: Option<&R>,
    ) -> Option<Vec<Pubkey>> {
        self.keys.fetch_add(1, Ordering::Relaxed);
        if !self.in_mem_only {
            self.flush(ix, false, None);
            self.disk.keys(ix)
        } else {
            let wc = self.write_cache[ix].read().unwrap();
            let mut result = Vec::with_capacity(wc.len());
            for i in wc.keys() {
                result.push(i.clone());
            }
            Some(result)
        }
    }
    pub fn values<R: RangeBounds<Pubkey>>(
        &self,
        ix: usize,
        _range: Option<&R>,
    ) -> Option<Vec<Vec<SlotT<V>>>> {
        if !self.in_mem_only {
            self.flush(ix, false, None);
            self.disk.values(ix)
        } else {
            let wc = self.write_cache[ix].read().unwrap();
            let mut result = Vec::with_capacity(wc.len());
            for i in wc.values() {
                result.push(i.instance.read().unwrap().data.slot_list.clone());
            }
            Some(result)
        }
    }

    pub fn num_buckets(&self) -> usize {
        self.bins
    }
    /*
        fn update_lazy_from_disk(pubkey: &Pubkey,        entry: &WriteCacheEntryArc<V>,
            mut new_value: AccountMapEntry<V>,
            reclaims: &mut SlotList<V>,
            reclaims_must_be_empty: bool,
        ) {
            // we have 1 entry for this pubkey. it was previously inserted, but we haven't checked to see if there is more info for this pubkey on disk already
            // todo
        }
    */

    fn upsert_item_in_cache(
        &self,
        entry: &WriteCacheEntryArc<V>,
        mut new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        reclaims_must_be_empty: bool,
        mut m1: Measure,
    ) {
        let mut instance = entry.instance.write().unwrap();

        if instance.must_do_lookup_from_disk {
            // this item exists in the cache, but we don't know if it exists on disk or not
            assert!(!instance.confirmed_not_on_disk);
            // we can call update on the item as it is. same slot will result in reclaims, different slot will result in slot list growing
            // must_do_lookup_from_disk should remain
        }

        instance.age = self.set_age_to_future();
        instance.dirty = true;
        if instance.confirmed_not_on_disk {
            self.using_empty_get.fetch_add(1, Ordering::Relaxed);
            instance.data.ref_count = 0;
            assert!(instance.data.slot_list.is_empty());
            instance.confirmed_not_on_disk = false; // we are inserted now if we were 'confirmed_not_on_disk' before. update_static below handles this fine
        }
        self.updates_in_cache.fetch_add(1, Ordering::Relaxed);

        let mut current = &mut instance.data;
        let (slot, new_entry) = new_value.slot_list.remove(0);
        let addref = WriteAccountMapEntry::update_slot_list(
            &mut current.slot_list,
            slot,
            new_entry,
            reclaims,
            reclaims_must_be_empty,
        );
        if addref {
            current.ref_count += 1;
        }
        m1.stop();
        self.update_cache_us
            .fetch_add(m1.as_ns(), Ordering::Relaxed);
    }

    pub fn upsert(
        &self,
        key: &Pubkey,
        mut new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        reclaims_must_be_empty: bool,
    ) {
        let must_do_lookup_from_disk = false;
        let confirmed_not_on_disk = false;
        /*
        let k = Pubkey::from_str("5x3NHJ4VEu2abiZJ5EHEibTc2iqW22Lc245Z3fCwCxRS").unwrap();
        if key == k {
            error!("{} {} upsert {}, {:?}", file!(), line!(), key, new_value);
        }
        */

        let ix = self.bucket_ix(&key);
        // try read lock first
        {
            let m1 = Measure::start("");
            let wc = &mut self.write_cache[ix].read().unwrap();
            let res = wc.get(&key);
            if let Some(occupied) = res {
                // already in cache, so call update_static function
                self.upsert_item_in_cache(
                    occupied,
                    new_value,
                    reclaims,
                    reclaims_must_be_empty,
                    m1,
                );
                return;
            }
        }

        // try write lock
        let mut m1 = Measure::start("");
        let wc = &mut self.write_cache[ix].write().unwrap();
        let res = wc.entry(key.clone());
        match res {
            HashMapEntry::Occupied(occupied) => {
                // already in cache, so call update_static function
                self.upsert_item_in_cache(
                    occupied.get(),
                    new_value,
                    reclaims,
                    reclaims_must_be_empty,
                    m1,
                );
                return;
            }
            HashMapEntry::Vacant(vacant) => {
                if !reclaims_must_be_empty {
                    if 1000 <= self.insert_without_lookup.fetch_add(1, Ordering::Relaxed) {
                        //panic!("upsert without reclaims");
                    }
                }
                if reclaims_must_be_empty && false {
                    // todo
                    // we don't have to go to disk to look this thing up yet
                    // not on disk, not in cache, so add to cache
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
                    return; // we can be satisfied that this index will be looked up later
                }
                let r = self.get_no_cache(&key); // maybe move this outside lock - but race conditions unclear
                if let Some(mut current) = r {
                    if 1000 <= self.updates.fetch_add(1, Ordering::Relaxed) {
                        //panic!("1000th update without loading");
                    }
                    let (slot, new_entry) = new_value.slot_list.remove(0);
                    let addref = WriteAccountMapEntry::update_slot_list(
                        &mut current.1,
                        slot,
                        new_entry,
                        reclaims,
                        reclaims_must_be_empty,
                    );
                    if addref {
                        current.0 += 1;
                    }
                    vacant.insert(self.allocate(
                        &current.1,
                        current.0,
                        true,
                        false,
                        must_do_lookup_from_disk,
                        confirmed_not_on_disk,
                    ));
                    m1.stop();
                    self.update_cache_us
                        .fetch_add(m1.as_ns(), Ordering::Relaxed);
                } else {
                    // not on disk, not in cache, so add to cache
                    self.inserts.fetch_add(1, Ordering::Relaxed);
                    vacant.insert(self.allocate(
                        &new_value.slot_list,
                        new_value.ref_count,
                        true,
                        true,
                        must_do_lookup_from_disk,
                        confirmed_not_on_disk,
                    ));
                }
            }
        }
    }

    fn in_cache(slot_list: &SlotList<V>) -> bool {
        slot_list.iter().any(|(_slot, info)| info.is_cached())
    }

    fn merge_slot_lists<'a>(
        slot_list: &'a mut SlotList<V>,
        ref_count: &'a mut RefCount,
        instance: &'a RwLockWriteGuard<'a, WriteCacheEntry<V>>,
    ) {
        let reclaims_must_be_empty = false;
        let mut _reclaims = Vec::default();
        for (slot, new_entry) in instance.data.slot_list.iter() {
            let addref = WriteAccountMapEntry::update_slot_list(
                slot_list,
                *slot,
                new_entry.clone(),
                &mut _reclaims,
                reclaims_must_be_empty,
            );
            if addref {
                *ref_count += 1;
            }
        }
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

        //let k = Pubkey::from_str("D5vcuUjK4uPSg1Cy9hoTPWCodFcLv6MyfWFNmRtbEofL").unwrap();
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
                    let wc = &mut self.write_cache[ix].write().unwrap();
                    wc.insert(
                        key.clone(),
                        self.allocate(
                            &result.0,
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
        } else {
            if !self.in_mem_only {
                // update
                //let current_value = current_value.map(|entry| (&entry.slot_list[..], entry.ref_count));
                self.disk.update(key, updatefn); //|_current| Some((v.0.clone(), v.1)));
            }
        }
    }

    fn allocate(
        &self,
        slot_list: &SlotList<V>,
        ref_count: RefCount,
        dirty: bool,
        insert: bool,
        must_do_lookup_from_disk: bool,
        confirmed_not_on_disk: bool,
    ) -> WriteCacheEntryArc<V> {
        assert!(!(insert && !dirty));
        WriteCacheEntryArc {
            instance: RwLock::new(WriteCacheEntry {
                data: AccountMapEntry {
                    slot_list: slot_list.clone(),
                    ref_count,
                },
                dirty: dirty, //AtomicBool::new(dirty),
                age: self.set_age_to_future(),
                insert,
                must_do_lookup_from_disk,
                confirmed_not_on_disk,
            }),
        }
    }

    fn upsert_in_cache<F>(&self, key: &Pubkey, updatefn: F, current_value: Option<&V2<V>>)
    where
        F: Fn(Option<(&[SlotT<V>], u64)>) -> Option<(Vec<SlotT<V>>, u64)>,
    {
        let must_do_lookup_from_disk = false;
        let confirmed_not_on_disk = false;
        let current_value = current_value.map(|entry| (&entry.slot_list[..], entry.ref_count));
        // we are an update
        let result = updatefn(current_value);
        if let Some(result) = result {
            // stick this in the write cache and flush it later
            let ix = self.bucket_ix(key);
            let wc = &mut self.write_cache[ix].write().unwrap();

            match wc.entry(key.clone()) {
                HashMapEntry::Occupied(mut occupied) => {
                    let gm = occupied.get_mut();
                    let insert = gm.instance.read().unwrap().insert;
                    *gm = self.allocate(
                        &result.0,
                        result.1,
                        true,
                        insert,
                        must_do_lookup_from_disk,
                        confirmed_not_on_disk,
                    );
                    drop(gm);
                    assert_eq!(occupied.get().instance.read().unwrap().insert, insert);
                    assert_eq!(
                        occupied
                            .get()
                            .instance
                            .read()
                            .unwrap()
                            .confirmed_not_on_disk,
                        confirmed_not_on_disk
                    );
                }
                HashMapEntry::Vacant(vacant) => {
                    vacant.insert(self.allocate(
                        &result.0,
                        result.1,
                        true,
                        true,
                        must_do_lookup_from_disk,
                        confirmed_not_on_disk,
                    ));
                    self.wait.notify_all(); // we have put something in the write cache that needs to be flushed sometime
                }
            }
        } else {
            panic!("expected value");
        }
    }

    pub fn update<F>(&self, key: &Pubkey, updatefn: F, current_value: Option<&V2<V>>)
    where
        F: Fn(Option<(&[SlotT<V>], u64)>) -> Option<(Vec<SlotT<V>>, u64)>,
    {
        /*
        let k = Pubkey::from_str("5x3NHJ4VEu2abiZJ5EHEibTc2iqW22Lc245Z3fCwCxRS").unwrap();
        if key == &k {
            error!("{} {} update {}", file!(), line!(), key);
        }
        */

        if current_value.is_none() {
            // we are an insert
            self.inserts.fetch_add(1, Ordering::Relaxed);

            if INSERT_CACHING {
                self.upsert_in_cache(key, updatefn, current_value);
            } else {
                self.update_no_cache(key, updatefn, current_value, false);
            }
        } else {
            if VERIFY_GET_ON_INSERT {
                assert!(self.get(key).is_some());
            }
            // if we have a previous value, then that item is currently open and locked, so it could not have been changed. Thus, this is an in-cache update as long as we are caching gets.
            self.updates_in_cache.fetch_add(1, Ordering::Relaxed);
            if UPDATE_CACHING {
                self.upsert_in_cache(key, updatefn, current_value);
            } else {
                self.update_no_cache(key, updatefn, current_value, false);
            }
        }
    }
    pub fn get_no_cache(&self, key: &Pubkey) -> Option<(u64, Vec<SlotT<V>>)> {
        let mut m1 = Measure::start("");
        if self.in_mem_only {
            return None;
        }
        let r = self.disk.get(key);
        m1.stop();
        self.get_disk_us.fetch_add(m1.as_ns(), Ordering::Relaxed);
        if r.is_some() {
            self.gets_from_disk.fetch_add(1, Ordering::Relaxed);
        } else {
            self.gets_from_disk_empty.fetch_add(1, Ordering::Relaxed);
        }
        r
    }

    fn set_age_to_future(&self) -> u8 {
        let current_age = self.current_age.load(Ordering::Relaxed);
        Self::add_age(current_age, DEFAULT_AGE)
    }

    fn get_caching(
        &self,
        instance: &RwLock<WriteCacheEntry<V>>,
        key: &Pubkey,
    ) -> Option<(u64, Vec<SlotT<V>>)> {
        let mut instance = instance.write().unwrap();
        instance.age = self.set_age_to_future();
        self.gets_from_cache.fetch_add(1, Ordering::Relaxed);
        if instance.confirmed_not_on_disk {
            return None; // does not really exist
        }
        if instance.must_do_lookup_from_disk {
            // we have inserted or updated this item, but we have never reconciled with the disk, so we have to do that now since a caller is requesting the complete item
            let r = self.get_no_cache(key);
            if let Some((mut ref_count, mut slot_list)) = r {
                Self::merge_slot_lists(&mut slot_list, &mut ref_count, &instance);
                instance.data.ref_count = ref_count;
                instance.data.slot_list = slot_list;
            }
            // else, we now know we were the only info that exists for this account, so we have all we need
            instance.must_do_lookup_from_disk = false;
        }

        let r = Some((instance.data.ref_count, instance.data.slot_list.clone()));
        return r;
    }

    pub fn range<R>(&self, ix: usize, range: Option<&R>) -> Vec<(Pubkey, SlotList<V>)>
    where
        R: RangeBounds<Pubkey>,
    {
        let mut m = Measure::start("");
        let r = if !self.in_mem_only {
            self.flush(ix, false, None);
            self.disk.range(ix, range).unwrap_or_default()
        } else {
            let wc = self.write_cache[ix].read().unwrap();
            let mut result = Vec::with_capacity(wc.len());
            for (k, v) in wc.iter() {
                let slot_list = &v.instance.read().unwrap().data.slot_list;
                if !slot_list.is_empty() {
                    result.push((*k, slot_list.clone()));
                }
            }
            result
        };
        m.stop();
        self.range_us.fetch_add(m.as_us(), Ordering::Relaxed);
        r
    }

    pub fn get(&self, key: &Pubkey) -> Option<(u64, Vec<SlotT<V>>)> {
        /*
        let k = Pubkey::from_str("5x3NHJ4VEu2abiZJ5EHEibTc2iqW22Lc245Z3fCwCxRS").unwrap();
        if key == &k {
            error!("{} {} get {}", file!(), line!(), key);
        }
        */
        let must_do_lookup_from_disk = false;
        let mut confirmed_not_on_disk = false;
        let ix = self.bucket_ix(key);
        {
            let mut m1 = Measure::start("");
            let wc = &mut self.write_cache[ix].read().unwrap();
            let res = wc.get(key);
            m1.stop();
            if let Some(res) = res {
                if GET_CACHING {
                    let res = self.get_caching(&res.instance, key);
                    self.get_cache_us.fetch_add(m1.as_ns(), Ordering::Relaxed);
                    return res;
                } else {
                    let instance = res.instance.read().unwrap();
                    self.gets_from_cache.fetch_add(1, Ordering::Relaxed);
                    return Some((instance.data.ref_count, instance.data.slot_list.clone()));
                }
            }
            if !GET_CACHING {
                return self.get_no_cache(key);
            }
        }
        // get caching
        {
            let mut m1 = Measure::start("");
            let wc = &mut self.write_cache[ix].write().unwrap();
            let res = wc.entry(key.clone());
            match res {
                HashMapEntry::Occupied(occupied) => {
                    let res = self.get_caching(&occupied.get().instance, key);
                    self.get_cache_us.fetch_add(m1.as_ns(), Ordering::Relaxed);
                    return res;
                }
                HashMapEntry::Vacant(vacant) => {
                    let r = self.get_no_cache(key);
                    if let Some(loaded) = &r {
                        vacant.insert(self.allocate(
                            &loaded.1,
                            loaded.0,
                            false,
                            false,
                            must_do_lookup_from_disk,
                            confirmed_not_on_disk,
                        ));
                    } else {
                        // we looked this up. it does not exist. let's insert a marker in the cache saying it doesn't exist. otherwise, we have to look it up again on insert!
                        confirmed_not_on_disk = true;
                        vacant.insert(self.allocate(
                            &SlotList::default(),
                            RefCount::MAX,
                            false,
                            false,
                            must_do_lookup_from_disk,
                            confirmed_not_on_disk,
                        ));
                    }
                    m1.stop();
                    self.update_cache_us
                        .fetch_add(m1.as_ns(), Ordering::Relaxed);
                    r
                }
            }
        }
    }
    fn addunref(&self, key: &Pubkey, _ref_count: RefCount, _slot_list: &Vec<SlotT<V>>, add: bool) {
        // todo: measure this and unref
        let ix = self.bucket_ix(key);
        let mut m1 = Measure::start("");
        let wc = &mut self.write_cache[ix].write().unwrap();

        let res = wc.entry(key.clone());
        m1.stop();
        self.get_cache_us.fetch_add(m1.as_ns(), Ordering::Relaxed);
        match res {
            HashMapEntry::Occupied(mut occupied) => {
                self.gets_from_cache.fetch_add(1, Ordering::Relaxed);
                let gm = occupied.get_mut();
                let mut instance = gm.instance.write().unwrap();
                if !instance.confirmed_not_on_disk {
                    if add {
                        instance.data.ref_count += 1;
                    } else {
                        instance.data.ref_count -= 1;
                    }
                }
            }
            HashMapEntry::Vacant(_vacant) => {
                if !self.in_mem_only {
                    self.gets_from_disk.fetch_add(1, Ordering::Relaxed);
                    if add {
                        self.disk.addref(key);
                    } else {
                        self.disk.unref(key);
                    }
                }
            }
        }
    }
    pub fn addref(&self, key: &Pubkey, ref_count: RefCount, slot_list: &Vec<SlotT<V>>) {
        self.addrefs.fetch_add(1, Ordering::Relaxed);
        self.addunref(key, ref_count, slot_list, true);
    }

    pub fn unref(&self, key: &Pubkey, ref_count: RefCount, slot_list: &Vec<SlotT<V>>) {
        self.unrefs.fetch_add(1, Ordering::Relaxed);
        self.addunref(key, ref_count, slot_list, false);
    }
    pub fn delete_key(&self, key: &Pubkey) {
        self.deletes.fetch_add(1, Ordering::Relaxed);
        let ix = self.bucket_ix(key);
        {
            let wc = &mut self.write_cache[ix].write().unwrap();
            wc.remove(key);
            //error!("remove: {}", key);
        }
        if !self.in_mem_only {
            self.disk.delete_key(key)
        }
    }
    pub fn distribution(&self) {}
    pub fn distribution2(&self) {
        let mut ct = 0;
        for i in 0..self.bins {
            ct += self.write_cache[i].read().unwrap().len();
        }
        let mut sum = 0;
        let mut min = usize::MAX;
        let mut max = 0;
        let mut sumd = 0;
        let mut mind = usize::MAX;
        let mut maxd = 0;
        let dist = if !self.in_mem_only {
            let dist = self.disk.distribution();
            for d in &dist.0 {
                let d = *d;
                sum += d;
                min = std::cmp::min(min, d);
                max = std::cmp::max(max, d);
            }
            for d in &dist.1 {
                let d = *d;
                sumd += d;
                mind = std::cmp::min(min, d);
                maxd = std::cmp::max(max, d);
            }
            dist
        } else {
            for d in &self.write_cache {
                let d = d.read().unwrap().len();
                sum += d;
                min = std::cmp::min(min, d);
                max = std::cmp::max(max, d);
            }
            (vec![], vec![], 0, 0, (0, 0, 0, 0))
        };
        datapoint_info!(
            "accounts_index",
            ("items_in_write_cache", ct, i64),
            ("min", min, i64),
            ("max", max, i64),
            ("sum", sum, i64),
            ("index_entries_allocated", dist.2, i64),
            ("mind", mind, i64),
            ("maxd", maxd, i64),
            ("sumd", sumd, i64),
            ("data_entries_allocated", dist.3, i64),
            ("data_q0", dist.4 .0, i64),
            ("data_q1", dist.4 .1, i64),
            ("data_q2", dist.4 .2, i64),
            ("data_q3", dist.4 .3, i64),
            (
                "updates_not_in_cache",
                self.updates.swap(0, Ordering::Relaxed),
                i64
            ),
            ("flush0", self.flush0.swap(0, Ordering::Relaxed), i64),
            ("flush1", self.flush1.swap(0, Ordering::Relaxed), i64),
            ("flush2", self.flush2.swap(0, Ordering::Relaxed), i64),
            ("flush3", self.flush3.swap(0, Ordering::Relaxed), i64),
            //("updates_not_in_cache", self.updates.load(Ordering::Relaxed), i64),
            (
                "updates_in_cache",
                self.updates_in_cache.swap(0, Ordering::Relaxed),
                i64
            ),
            ("inserts", self.inserts.swap(0, Ordering::Relaxed), i64),
            (
                "inserts_without_checking_disk",
                self.inserts_without_checking_disk
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            (
                "using_empty_get",
                self.using_empty_get.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "insert_without_lookup",
                self.insert_without_lookup.swap(0, Ordering::Relaxed),
                i64
            ),
            //("insert_without_lookup", self.insert_without_lookup.load(Ordering::Relaxed), i64),
            (
                "gets_from_disk_empty",
                self.gets_from_disk_empty.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "gets_from_disk",
                self.gets_from_disk.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "gets_from_cache",
                self.gets_from_cache.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_index_resizes",
                self.disk.stats.index.resizes.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_index_resize_us",
                self.disk.stats.index.resize_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_index_max",
                *self.disk.stats.index.max_size.lock().unwrap(),
                i64
            ),
            (
                "bucket_index_new_file_us",
                self.disk.stats.index.new_file_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_flush_file_us_us",
                self.disk
                    .stats
                    .index
                    .flush_file_us
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_mmap_us",
                self.disk.stats.index.mmap_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_data_resizes",
                self.disk.stats.data.resizes.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_data_resize_us",
                self.disk.stats.data.resize_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bucket_data_max",
                *self.disk.stats.data.max_size.lock().unwrap(),
                i64
            ),
            (
                "bucket_data_new_file_us",
                self.disk.stats.data.new_file_us.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "flushes",
                self.write_cache_flushes.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "bin_flush_calls",
                self.bucket_flush_calls.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_purges",
                self.get_purges.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_disk_us",
                self.get_disk_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "update_dist_us",
                self.update_dist_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "get_cache_us",
                self.get_cache_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            (
                "update_cache_us",
                self.update_cache_us.swap(0, Ordering::Relaxed) / 1000,
                i64
            ),
            ("addrefs", self.addrefs.swap(0, Ordering::Relaxed), i64),
            ("unrefs", self.unrefs.swap(0, Ordering::Relaxed), i64),
            ("range", self.range.swap(0, Ordering::Relaxed), i64),
            ("range_us", self.range_us.swap(0, Ordering::Relaxed), i64),
            ("keys", self.keys.swap(0, Ordering::Relaxed), i64),
            ("get", self.updates.swap(0, Ordering::Relaxed), i64),
            //("buckets", self.num_buckets(), i64),
        );
    }
}
