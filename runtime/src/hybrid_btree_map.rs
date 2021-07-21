use crate::accounts_index::AccountMapEntry;
use crate::accounts_index::{AccountMapEntrySerialize, RefCount, SlotList, BINS};
use crate::pubkey_bins::PubkeyBinCalculator16;
use log::*;
use solana_bucket_map::bucket_map::BucketMap;
use solana_sdk::clock::Slot;
use solana_sdk::pubkey::Pubkey;
use std::borrow::Borrow;
use std::collections::btree_map::BTreeMap;
use std::fmt::Debug;
use std::marker::{Send, Sync};
use std::ops::Bound;
use std::ops::{Range, RangeBounds};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Instant;
type K = Pubkey;
use std::collections::{hash_map::Entry as HashMapEntry, HashMap};
use std::ops::Bound::{Excluded, Included, Unbounded};

#[derive(Clone, Debug)]
pub struct HybridAccountEntry<V: Clone + Debug> {
    entry: V,
    //exists_on_disk: bool,
}
//type V2<T: Clone + Debug> = HybridAccountEntry<T>;
type V2<T> = AccountMapEntry<T>;
/*
trait RealEntry<T: Clone + Debug> {
    fn real_entry(&self) -> T;
}

impl<T:Clone + Debug> RealEntry<T> for T {
    fn real_entry(&self) -> T
    {
        self
    }
}
*/

#[derive(Debug, Default)]
pub struct PubkeyRange {
    pub start_pubkey_include: Option<Pubkey>,
    pub start_pubkey_exclude: Option<Pubkey>,
    pub end_pubkey_exclude: Option<Pubkey>,
}

pub trait Rox: Debug + Send + Sync {
    fn get(&self, pubkey: &Pubkey) -> Option<AccountMapEntrySerialize>;
    fn insert(&self, pubkey: &Pubkey, value: &AccountMapEntrySerialize);
    fn update(&self, pubkey: &Pubkey, value: &AccountMapEntrySerialize);
    fn delete(&self, pubkey: &Pubkey);
    fn addref(&self, pubkey: &Pubkey, info: &SlotList<AccountInfo>);
    fn unref(&self, pubkey: &Pubkey, info: &SlotList<AccountInfo>);
    fn keys(&self, range: Option<&PubkeyRange>) -> Option<Vec<Pubkey>>;
    fn values(&self, range: Option<&PubkeyRange>)
        -> Option<Vec<AccountMapEntrySerialize>>;
}
use crate::accounts_db::AccountInfo;
pub trait Guts: Sized {
    fn get_info(&self) -> AccountInfo;
    fn get_info2(info: &SlotList<Self>) -> SlotList<AccountInfo>;
    fn get_copy(info: &AccountInfo) -> Self;
    fn get_copy2(info: &SlotList<AccountInfo>) -> SlotList<Self>;
}

impl Guts for AccountInfo {
    fn get_info(&self) -> AccountInfo {
        self.clone()
    }
    fn get_copy(info: &AccountInfo) -> Self {
        info.clone()
    }
    fn get_info2(info: &SlotList<Self>) -> SlotList<AccountInfo>
    {
        info.clone()
    }
    fn get_copy2(info: &SlotList<AccountInfo>) -> SlotList<Self>
    {
        info.clone()
    }
}

pub type SlotT<T> = (Slot, T);

pub type WriteCache<V> = HashMap<Pubkey, V>;
use crate::waitable_condvar::WaitableCondvar;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Duration;
#[derive(Debug)]
pub struct BucketMapWriteHolder<V> {
    pub disk: BucketMap<SlotT<V>>,
    pub write_cache: Vec<RwLock<WriteCache<V2<V>>>>,
    pub write_cache_flushes: AtomicU64,
    pub gets: AtomicU64,
    pub updates: AtomicU64,
    pub inserts: AtomicU64,
    pub deletes: AtomicU64,
    pub bins: usize,
    pub wait: WaitableCondvar,
    pub db: Arc<RwLock<Option<Arc<Box<dyn Rox>>>>>,
}

pub const use_rox: bool = true;

impl<V: 'static + Clone + Debug + Guts> BucketMapWriteHolder<V> {
    pub fn set_account_index_db(&self, db: Arc<Box<dyn Rox>>) {
        *self.db.write().unwrap() = Some(db);
    }

    pub fn bg_flusher(&self, exit: Arc<AtomicBool>) {
        let mut found_one = false;
        let mut last = Instant::now();
        loop {
            if last.elapsed().as_millis() > 5000 {
                self.distribution2();
                last = Instant::now();
            }
            if exit.load(Ordering::Relaxed) {
                break;
            }
            if !found_one && self.wait.wait_timeout(Duration::from_millis(500)) {
                continue;
            }
            found_one = false;
            for ix in 0..self.bins {
                if !self.write_cache[ix].read().unwrap().is_empty() {
                    found_one = true;
                    let mut wc = self.write_cache[ix].write().unwrap();
                    self.write_cache_flushes
                        .fetch_add(wc.len() as u64, Ordering::Relaxed);
                    for (k, v) in wc.iter() {
                        self.disk
                            .update(k, |_current| Some((v.slot_list.clone(), v.ref_count)));
                    }
                    wc.clear();
                }
            }
        }
    }
    fn new(bucket_map: BucketMap<SlotT<V>>) -> Self {
        let write_cache_flushes = AtomicU64::new(0);
        let gets = AtomicU64::new(0);
        let updates = AtomicU64::new(0);
        let inserts = AtomicU64::new(0);
        let deletes = AtomicU64::new(0);
        let mut write_cache = vec![];
        let bins = bucket_map.num_buckets();
        write_cache = (0..bins)
            .map(|i| RwLock::new(WriteCache::default()))
            .collect::<Vec<_>>();
        let wait = WaitableCondvar::default();
        let db = Arc::new(RwLock::new(None));

        Self {
            disk: bucket_map,
            write_cache,
            bins,
            wait,
            gets,
            deletes,
            write_cache_flushes,
            updates,
            inserts,
            db,
        }
    }
    pub fn flush(&self, ix: usize) {
        if ix < self.write_cache.len() {
            let wc = &mut self.write_cache[ix].write().unwrap();
            self.write_cache_flushes
                .fetch_add(wc.len() as u64, Ordering::Relaxed);
            for (k, v) in wc.iter() {
                self.disk
                    .update(k, |_previous| Some((v.slot_list.clone(), v.ref_count)));
            }
            wc.clear();
        }
    }
    pub fn bucket_len(&self, ix: usize) -> u64 {
        self.disk.bucket_len(ix)
    }
    pub fn bucket_ix(&self, key: &Pubkey) -> usize {
        if use_rox {
            return 0;
        }
        self.disk.bucket_ix(key)
    }
    pub fn keys<R: RangeBounds<Pubkey>>(&self, ix: usize, range: Option<&R>) -> Option<Vec<Pubkey>> {
        if use_rox {
            let mut range_use = PubkeyRange::default();
            if let Some(range) = range {
                match range.start_bound() {
                    Included(pubkey) => {range_use.start_pubkey_include = Some(pubkey.clone());},
                    Excluded(pubkey) => {range_use.start_pubkey_exclude = Some(pubkey.clone());},
                    Unbounded => {},
                }
                match range.end_bound() {
                    Included(pubkey) => {panic!("");},
                    Excluded(pubkey) => {range_use.end_pubkey_exclude = Some(pubkey.clone());},
                    Unbounded => {},
                }
            }
            return self.db
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .keys(Some(&range_use)); // todo range
        }
        self.disk.keys(ix)
    }
    pub fn values<R: RangeBounds<Pubkey>>(&self, ix: usize, range: Option<&R>) -> Option<Vec<Vec<SlotT<V>>>> {
        if use_rox {
            return self.db
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .values(None).map(|x| x.into_iter().map(|x| V::get_copy2(&x.slot_list)).collect());
        }

        // only valid when write cache is empty
        assert!(self.write_cache[ix].read().unwrap().is_empty());
        self.disk.values(ix)
    }

    pub fn num_buckets(&self) -> usize {
        if use_rox {
            return 1;
        }
        self.disk.num_buckets()
    }
    pub fn update<F>(&self, key: &Pubkey, updatefn: F, current_value: Option<&V2<V>>)
    where
        F: Fn(Option<(&[SlotT<V>], u64)>) -> Option<(Vec<SlotT<V>>, u64)>,
    {
        if current_value.is_none() {
            // we are an insert
            self.inserts.fetch_add(1, Ordering::Relaxed);

            if use_rox {
                let result = updatefn(None).unwrap();
                let result = AccountMapEntrySerialize {
                    slot_list: result
                        .0
                        .into_iter()
                        .map(|(slot, info)| (slot, info.get_info().clone()))
                        .collect(), // bad
                    ref_count: result.1,
                };
                self.db
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .insert(key, &result);
                return;
            }
            if true {
                // send straight to disk. if we try to keep it in the write cache, then 'keys' will be incorrect
                self.disk.update(key, updatefn);
            } else {
                let result = updatefn(None);
                if let Some(result) = result {
                    // stick this in the write cache and flush it later
                    let ix = self.disk.bucket_ix(key);
                    let wc = &mut self.write_cache[ix].write().unwrap();
                    wc.insert(
                        key.clone(),
                        AccountMapEntry {
                            slot_list: result.0,
                            ref_count: result.1,
                        },
                    );
                } else {
                    panic!("should have returned a value from updatefn");
                }
            }
        } else {
            // update
            if use_rox {
                let entry = current_value.unwrap();
                let current_value = Some((&entry.slot_list[..], entry.ref_count));
                let result = updatefn(current_value).unwrap();
                let result = AccountMapEntrySerialize {
                    slot_list: result
                        .0
                        .into_iter()
                        .map(|(slot, info)| (slot, info.get_info().clone()))
                        .collect(), // bad
                    ref_count: result.1,
                };
                self.db
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .update(key, &result);
                return;
            }

            self.updates.fetch_add(1, Ordering::Relaxed);
            let entry = current_value.unwrap();
            let current_value = Some((&entry.slot_list[..], entry.ref_count));
            // we are an update
            let result = updatefn(current_value);
            if let Some(result) = result {
                // stick this in the write cache and flush it later
                let ix = self.disk.bucket_ix(key);
                let wc = &mut self.write_cache[ix].write().unwrap();

                match wc.entry(key.clone()) {
                    HashMapEntry::Occupied(mut occupied) => {
                        let mut gm = occupied.get_mut();
                        gm.slot_list = result.0;
                        gm.ref_count = result.1;
                    }
                    HashMapEntry::Vacant(vacant) => {
                        vacant.insert(AccountMapEntry {
                            slot_list: result.0,
                            ref_count: result.1,
                        });
                        self.wait.notify_all(); // we have put something in the write cache that needs to be flushed sometime
                    }
                }
            }
        }
    }
    pub fn get(&self, key: &Pubkey) -> Option<(u64, Vec<SlotT<V>>)> {
        self.gets.fetch_add(1, Ordering::Relaxed);

        if use_rox {
            let result = self.db.read().unwrap().as_ref().unwrap().get(key);
            let result = result.map(|result| {
                (
                    result.ref_count,
                    result
                        .slot_list
                        .into_iter()
                        .map(|(slot, info)| (slot, V::get_copy(&info)))
                        .collect(), // bad
                )
            });
            return result;
        }

        let ix = self.disk.bucket_ix(key);
        let wc = &mut self.write_cache[ix].read().unwrap();
        let res = wc.get(key);
        if let Some(res) = res {
            Some((res.ref_count, res.slot_list.clone()))
        } else {
            self.disk.get(key)
        }
    }
    pub fn addref(&self, key: &Pubkey, slot_list: &Vec<SlotT<V>>) {
        // todo: measure this and unref
        if use_rox {
            self.db.read().unwrap().as_ref().unwrap().addref(key, &V::get_info2(&slot_list));
            return;
            }
        let ix = self.disk.bucket_ix(key);
        let wc = &mut self.write_cache[ix].write().unwrap();

        match wc.entry(key.clone()) {
            HashMapEntry::Occupied(mut occupied) => {
                let mut gm = occupied.get_mut();
                gm.ref_count += 1;
            }
            HashMapEntry::Vacant(vacant) => {
                self.disk.addref(key);
            }
        }
    }

    pub fn unref(&self, key: &Pubkey, slot_list: &Vec<SlotT<V>>) {
        if use_rox {
            self.db.read().unwrap().as_ref().unwrap().unref(key, &V::get_info2(&slot_list));
            return;
            }
        let ix = self.disk.bucket_ix(key);
        let wc = &mut self.write_cache[ix].write().unwrap();

        match wc.entry(key.clone()) {
            HashMapEntry::Occupied(mut occupied) => {
                let mut gm = occupied.get_mut();
                gm.ref_count -= 1;
            }
            HashMapEntry::Vacant(vacant) => {
                self.disk.unref(key);
            }
        }
    }
    fn delete_key(&self, key: &Pubkey) {
        if use_rox {
            self.db.read().unwrap().as_ref().unwrap().delete(key);
            return;
            }
        self.deletes.fetch_add(1, Ordering::Relaxed);
        let ix = self.disk.bucket_ix(key);
        {
            let wc = &mut self.write_cache[ix].write().unwrap();
            wc.remove(key);
        }
        self.disk.delete_key(key)
    }
    pub fn distribution(&self) {}
    pub fn distribution2(&self) {
        if use_rox {
            return;
        }
            let mut ct = 0;
        for i in 0..self.bins {
            ct += self.write_cache[i].read().unwrap().len();
        }
        error!("{} items in write cache", ct);
        let dist = self.disk.distribution();
        let mut sum = 0;
        let mut min = usize::MAX;
        let mut max = 0;
        for d in &dist {
            let d = *d;
            sum += d;
            min = std::cmp::min(min, d);
            max = std::cmp::max(max, d);
        }
        datapoint_info!(
            "AccountIndexLayer",
            ("items_in_write_cache", ct, i64),
            ("min", min, i64),
            ("max", max, i64),
            ("sum", sum, i64),
            ("updates", self.updates.swap(0, Ordering::Relaxed), i64),
            ("inserts", self.inserts.swap(0, Ordering::Relaxed), i64),
            ("deletes", self.deletes.swap(0, Ordering::Relaxed), i64),
            ("gets", self.gets.swap(0, Ordering::Relaxed), i64),
            (
                "flushes",
                self.write_cache_flushes.swap(0, Ordering::Relaxed),
                i64
            ),
            //("buckets", self.num_buckets(), i64),
        );
    }
}

#[derive(Debug)]
pub struct HybridBTreeMap<V: 'static + Clone + Debug> {
    in_memory: BTreeMap<K, V2<V>>,
    disk: Arc<BucketMapWriteHolder<V>>,
    bin_index: usize,
    bins: usize,
}

// TODO: we need a bit for 'exists on disk' for updates
/*
impl<V: Clone + Debug> Default for HybridBTreeMap<V> {
    /// Creates an empty `BTreeMap`.
    fn default() -> HybridBTreeMap<V> {
        Self {
            in_memory: BTreeMap::default(),
            disk: BucketMap::new_buckets(PubkeyBinCalculator16::log_2(BINS as u32) as u8),
        }
    }
}
*/

/*
impl<'a, K: 'a, V: 'a> Iterator for HybridBTreeMap<'a, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<(&'a K, &'a V)> {
        if self.length == 0 {
            None
        } else {
            self.length -= 1;
            Some(unsafe { self.range.inner.next_unchecked() })
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.length, Some(self.length))
    }

    fn last(mut self) -> Option<(&'a K, &'a V)> {
        self.next_back()
    }

    fn min(mut self) -> Option<(&'a K, &'a V)> {
        self.next()
    }

    fn max(mut self) -> Option<(&'a K, &'a V)> {
        self.next_back()
    }
}
*/

pub enum HybridEntry<'a, V: 'static + Clone + Debug + Guts> {
    /// A vacant entry.
    Vacant(HybridVacantEntry<'a, V>),

    /// An occupied entry.
    Occupied(HybridOccupiedEntry<'a, V>),
}

pub struct Keys {
    keys: Vec<K>,
    index: usize,
}

impl Iterator for Keys {
    type Item = Pubkey;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.keys.len() {
            None
        } else {
            let r = Some(self.keys[self.index]);
            self.index += 1;
            r
        }
    }
}

pub struct Values<V: Clone + std::fmt::Debug> {
    values: Vec<(SlotList<V>)>,
    index: usize,
}

impl<V: Clone + std::fmt::Debug> Iterator for Values<V> {
    type Item = V2<V>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.values.len() {
            None
        } else {
            let r = Some(AccountMapEntry {
                slot_list: self.values[self.index].clone(),
                ref_count: RefCount::MAX, // todo: no clone
            });
            self.index += 1;
            r
        }
    }
}

pub struct HybridOccupiedEntry<'a, V: 'static + Clone + Debug + Guts> {
    pubkey: Pubkey,
    entry: V2<V>,
    map: &'a HybridBTreeMap<V>,
}
pub struct HybridVacantEntry<'a, V: 'static + Clone + Debug + Guts> {
    pubkey: Pubkey,
    map: &'a HybridBTreeMap<V>,
}

impl<'a, V: 'a + Clone + Debug + Guts> HybridOccupiedEntry<'a, V> {
    pub fn get(&self) -> &V2<V> {
        &self.entry
    }
    pub fn update(&mut self, new_data: &SlotList<V>, new_rc: Option<RefCount>) {
        //error!("update: {}", self.pubkey);
        self.map.disk.update(
            &self.pubkey,
            |previous| {
                if previous.is_some() {
                    //error!("update {} to {:?}", self.pubkey, new_data);
                }
                Some((new_data.clone(), new_rc.unwrap_or(self.entry.ref_count)))
                // TODO no clone here
            },
            Some(&self.entry),
        );
        let g = self.map.disk.get(&self.pubkey).unwrap();
        assert_eq!(format!("{:?}", g.1), format!("{:?}", new_data));
    }
    pub fn addref(&mut self) {
        self.entry.ref_count += 1;
        let result = self.map.disk.addref(&self.pubkey, &self.entry.slot_list);
        //error!("addref: {}, {}, {:?}", self.pubkey, self.entry.ref_count(), result);
    }
    pub fn unref(&mut self) {
        self.entry.ref_count -= 1;
        let result = self.map.disk.unref(&self.pubkey, &self.entry.slot_list);
        //error!("addref: {}, {}, {:?}", self.pubkey, self.entry.ref_count(), result);
    }
    /*
    pub fn get_mut(&mut self) -> &mut V2<V> {
        self.entry.get_mut()
    }
    */
    pub fn key(&self) -> &K {
        &self.pubkey
    }
    pub fn remove(self) {
        self.map.disk.delete_key(&self.pubkey)
    }
}

impl<'a, V: 'a + Clone + Debug + Guts> HybridVacantEntry<'a, V> {
    pub fn insert(self, value: V2<V>) {
        // -> &'a mut V2<V> {
        /*
        let value = V2::<V> {
            entry: value,
            //exists_on_disk: false,
        };
        */
        //let mut sl = SlotList::default();
        //std::mem::swap(&mut sl, &mut value.slot_list);
        self.map.disk.update(
            &self.pubkey,
            |_previous| {
                Some((value.slot_list.clone() /* todo bad */, value.ref_count))
            },
            None,
        );
    }
}

impl<V: 'static + Clone + Debug + Guts> HybridBTreeMap<V> {
    /// Creates an empty `BTreeMap`.
    pub fn new(
        bucket_map: &Arc<BucketMapWriteHolder<V>>,
        bin_index: usize,
        bins: usize,
    ) -> HybridBTreeMap<V> {
        Self {
            in_memory: BTreeMap::default(),
            disk: bucket_map.clone(),
            bin_index,
            bins: bucket_map.num_buckets(),
        }
    }
    pub fn new_bucket_map() -> Arc<BucketMapWriteHolder<V>> {
        let mut buckets = PubkeyBinCalculator16::log_2(BINS as u32) as u8; // make more buckets to try to spread things out
                                                                           // 15 hopefully avoids too many files open problem
        buckets = std::cmp::min(buckets + 11, 15); // max # that works with open file handles and such
        error!("creating: {} for {}", buckets, BINS);
        Arc::new(BucketMapWriteHolder::new(BucketMap::new_buckets(buckets)))
    }

    pub fn flush(&mut self) {
        let num_buckets = self.disk.num_buckets();
        let mystart = num_buckets * self.bin_index / self.bins;
        let myend = num_buckets * (self.bin_index + 1) / self.bins;
        (mystart..myend).for_each(|ix| {
            self.disk.flush(ix);
        });

        /*
        {
            // put entire contents of this map into the disk backing
            let mut keys = Vec::with_capacity(self.in_memory.len());
            for k in self.in_memory.keys() {
                keys.push(k);
            }
            self.disk.update_batch(&keys[..], |previous, key, orig_i| {
                let item = self.in_memory.get(key);
                item.map(|item| (item.slot_list.clone(), item.ref_count()))
            });
            self.in_memory.clear();
        }*/
    }
    pub fn set_account_index_db(&self, db: Arc<Box<dyn Rox>>) {
        self.disk.set_account_index_db(db);
    }

    pub fn distribution(&self) {
        self.disk.distribution();
    }
    fn bound<'a, T>(bound: Bound<&'a T>, unbounded: &'a T) -> &'a T {
        match bound {
            Bound::Included(b) | Bound::Excluded(b) => b,
            _ => unbounded,
        }
    }
    pub fn range<R>(&self, range: Option<R>) -> Keys
    where
        R: RangeBounds<Pubkey>,
    {
        let num_buckets = self.disk.num_buckets();
        if self.bin_index != 0 && num_buckets == 1 {
            return Keys {keys: vec![], index: 0};
        }
        let mut start = 0;
        let mut end = num_buckets;
        if let Some(range) = &range {
            start = self
                .disk
                .bucket_ix(Self::bound(range.start_bound(), &Pubkey::default()));
            // end is exclusive, so it is end + 1 we care about here
            end = std::cmp::min(
                num_buckets,
                1 + self
                    .disk
                    .bucket_ix(Self::bound(range.end_bound(), &Pubkey::new(&[0xff; 32]))),
            ); // ugly
        }
        let len = (start..end)
            .into_iter()
            .map(|ix| self.disk.bucket_len(ix) as usize)
            .sum::<usize>();

        let mystart = num_buckets * self.bin_index / self.bins;
        let myend = num_buckets * (self.bin_index + 1) / self.bins;
        start = std::cmp::max(start, mystart);
        end = std::cmp::min(end, myend);
        let mut keys = Vec::with_capacity(len);
        (start..end).into_iter().for_each(|ix| {
            let ks = self.disk.keys(ix, range.as_ref()).unwrap_or_default();
            for k in ks.into_iter() {
                if range.is_none() || range.as_ref().unwrap().contains(&k) {
                    keys.push(k);
                }
            }
        });
        keys.sort_unstable();
        Keys { keys, index: 0 }
    }

    pub fn keys2(&self) -> Keys { // used still?
        let num_buckets = self.disk.num_buckets();
        let start = num_buckets * self.bin_index / self.bins;
        let end = num_buckets * (self.bin_index + 1) / self.bins;
        let len = (start..end)
            .into_iter()
            .map(|ix| self.disk.bucket_len(ix) as usize)
            .sum::<usize>();
        let mut keys = Vec::with_capacity(len);
        let len = (start..end)
            .into_iter()
            .for_each(|ix| keys.append(&mut self.disk.keys(ix, None::<&Range<Pubkey>>).unwrap_or_default()));
        keys.sort_unstable();
        Keys { keys, index: 0 }
    }
    pub fn values(&self) -> Values<V> {
        let num_buckets = self.disk.num_buckets();
        if self.bin_index != 0 && num_buckets == 1 {
            return Values { values: vec![], index: 0 };
        }
        // todo: this may be unsafe if we are asking for things with an update cache active. thankfully, we only call values at startup right now
        let start = num_buckets * self.bin_index / self.bins;
        let end = num_buckets * (self.bin_index + 1) / self.bins;
        let len = (start..end)
            .into_iter()
            .map(|ix| self.disk.bucket_len(ix) as usize)
            .sum::<usize>();
        let mut values = Vec::with_capacity(len);
        (start..end)
            .into_iter()
            .for_each(|ix| values.append(&mut self.disk.values(ix, None::<&Range<Pubkey>>).unwrap_or_default()));
        //error!("getting values: {}, bin: {}, bins: {}, start: {}, end: {}", values.len(), self.bin_index, self.bins, start, end);
        //keys.sort_unstable();
        error!("getting values: {}, {}, {}", values.len(), start, end);
        Values { values, index: 0 }
    }
    pub fn len_inaccurate(&self) -> usize {
        1 // ??? wrong
          //self.in_memory.len()
    }
    pub fn entry(&mut self, key: K) -> HybridEntry<'_, V> {
        match self.disk.get(&key) {
            Some(entry) => HybridEntry::Occupied(HybridOccupiedEntry {
                pubkey: key,
                entry: AccountMapEntry::<V> {
                    slot_list: entry.1,
                    ref_count: entry.0,
                },
                map: self,
            }),
            None => HybridEntry::Vacant(HybridVacantEntry {
                pubkey: key,
                map: self,
            }),
        }
    }

    pub fn insert(&mut self, key: K, value: V2<V>) {
        match self.entry(key) {
            HybridEntry::Occupied(occupied) => {
                panic!("");
            }
            HybridEntry::Vacant(vacant) => vacant.insert(value),
        }
    }

    pub fn get(&self, key: &K) -> Option<V2<V>> {
        let lookup = || {
            let disk = self.disk.get(key);
            disk.map(|disk| AccountMapEntry {
                ref_count: disk.0,
                slot_list: disk.1,
            })
        };

        if true {
            lookup()
        } else {
            let in_mem = self.in_memory.get(key);
            match in_mem {
                Some(in_mem) => Some(in_mem.clone()),
                None => {
                    // we have to load this into the in-mem cache so we can get a ref_count, if nothing else
                    lookup()
                    /*
                    disk.map(|item| {
                        self.in_memory.entry(*key).map(|entry| {

                        }
                    })*/
                }
            }
        }
    }
    pub fn remove(&mut self, key: &K) {
        self.disk.delete_key(key); //.map(|x| x.entry)
    }
}
