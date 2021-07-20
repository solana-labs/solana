//! BucketMap is a mostly contention free concurrent map backed by MmapMut

use crate::data_bucket::DataBucket;
use log::*;
use rand::thread_rng;
use rand::Rng;
use solana_sdk::pubkey::Pubkey;
use std::collections::hash_map::DefaultHasher;
use std::convert::TryInto;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::{
    fs,
    sync::{atomic::Ordering},
};
pub struct BucketMap<T> {
    buckets: Vec<RwLock<Option<Bucket<T>>>>,
    drives: Arc<Vec<PathBuf>>,
    bits: u8,
}

impl<T> std::fmt::Debug for BucketMap<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BucketMap TODO")?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum BucketMapError {
    DataNoSpace((u64, u8)),
    IndexNoSpace(u8),
}
/*
impl<T: Clone + std::fmt::Debug> Default for BucketMap<T>  {
    fn default() -> Self {
        Self::new(1, Self::default_drives())
    }
}
*/

impl<T: Clone + std::fmt::Debug> BucketMap<T> {
    pub fn new(mut num_buckets_pow2: u8, drives: Arc<Vec<PathBuf>>) -> Self {
        Self::delete_previous(&drives);
        let count = 1 << num_buckets_pow2;
        let mut buckets = Vec::with_capacity(count);
        buckets.resize_with(count, || RwLock::new(None));
        error!("# buckets: {}", count);
        Self {
            buckets,
            drives,
            bits: num_buckets_pow2,
        }
    }

    pub fn distribution(&self) -> Vec<usize> {
        let mut sizes = vec![];
        for ix in 0..self.num_buckets() {
            let mut len =0;
            if let Ok(bucket) = self.buckets[ix].read() {
                if let Some(bucket) = bucket.as_ref() {
                    len = bucket.index.used.load(Ordering::Relaxed) as usize;
                }
            }
            sizes.push(len);
        }
        sizes
    }

    fn delete_previous(drives: &Arc<Vec<PathBuf>>) {
        for d in drives.iter() {
            if d.is_dir() {
                let dir = fs::read_dir(d.clone());
                if let Ok(dir) = dir {
                    for entry in dir {
                        if let Ok(entry) = entry {
                            let result_ = fs::remove_file(entry.path().clone());
                            if result_.is_err() {
                                error!("failed to remove: {:?}", entry.path());
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn default_drives() -> Arc<Vec<PathBuf>> {
        let tmpdir2 = PathBuf::from("accounts_index_buckets");
        error!("folder: {:?}", tmpdir2);
        let paths: Vec<PathBuf> = [tmpdir2]
            .iter()
            .filter(|x| std::fs::create_dir_all(x).is_ok())
            .cloned()
            .collect();
        assert!(!paths.is_empty());
        Arc::new(paths)
    }
    pub fn new_buckets(num_buckets_pow2: u8) -> Self {
        Self::new(num_buckets_pow2, Self::default_drives())
    }
    pub fn num_buckets(&self) -> usize {
        self.buckets.len()
    }
    pub fn keys(&self, ix: usize) -> Option<Vec<Pubkey>> {
        Some(self.buckets[ix].read().unwrap().as_ref()?.keys())
    }
    pub fn bucket_len(&self, ix: usize) -> u64 {
        self.buckets[ix].read().unwrap().as_ref().map(|entry| entry.index.used.load(Ordering::Relaxed)).unwrap_or_default()
    }
    pub fn values(&self, ix: usize) -> Option<Vec<Vec<T>>> {
        Some(self.buckets[ix].read().unwrap().as_ref()?.values())
    }

    pub fn read(&self) -> Option<&Self> {
        Some(self)
    }

    pub fn write(&self) -> Option<&Self> {
        Some(self)
    }

    pub fn get(&self, key: &Pubkey) -> Option<(u64, Vec<T>)> {
        let ix = self.bucket_ix(key);
        self.buckets[ix]
            .read()
            .unwrap()
            .as_ref()
            .and_then(|bucket| {
                bucket.find_entry(key).and_then(|(elem, _)| {
                    elem.read_value(bucket)
                        .and_then(|slice| Some((elem.ref_count, slice.0.to_vec())))
                })
            })
    }

    pub fn read_value(&self, key: &Pubkey) -> Option<(Vec<T>, u64)> {
        let ix = self.bucket_ix(key);
        self.buckets[ix].read().unwrap().as_ref().and_then(|x| {
            let slice = x.read_value(key)?;
            Some((slice.0.to_vec(), slice.1))
        })
    }

    pub fn delete_key(&self, key: &Pubkey) {
        let ix = self.bucket_ix(key);
        let mut bucket = self.buckets[ix].write().unwrap();
        if !bucket.is_none() {
            bucket.as_mut().unwrap().delete_key(key);
        }
    }

    pub fn update_batch<'a, F>(&'a self, keys: &[&Pubkey], updatefn: F)
    where
        F: Fn(Option<(&[T], u64)>, &Pubkey, usize) -> Option<(Vec<T>, u64)>,
    {
        let num_buckets = self.num_buckets();
        // group by bucket
        let mut per_bucket = vec![vec![]; num_buckets];
        for key in keys.iter() {
            let ix = self.bucket_ix(key);
            per_bucket[ix].push(key);
        }

        for ix in 0..num_buckets {
            let per_bucket = &per_bucket[ix];
            if per_bucket.is_empty() {
                continue;
            }
            let mut bucket = self.buckets[ix].write().unwrap();
            if bucket.is_none() {
                *bucket = Some(Bucket::new(self.drives.clone()));
            }
            let bucket = bucket.as_mut().unwrap();
            for key in per_bucket {
                let current = None;// will never find this key: bucket.read_value(key);

                let new = updatefn(current, key, usize::MAX);
                if new.is_none() {
                    bucket.delete_key(key);
                    continue;
                }
                let (new, refct) = new.unwrap();
                loop {
                    let rv = bucket.try_write(key, &new, refct);
                    match rv {
                        Err(BucketMapError::DataNoSpace(sz)) => {
                            debug!("GROWING SPACE {:?}", sz);
                            bucket.grow_data(sz);
                            continue;
                        }
                        Err(BucketMapError::IndexNoSpace(sz)) => {
                            debug!("GROWING INDEX {}", sz);
                            bucket.grow_index(sz);
                            continue;
                        }
                        Ok(()) => {
                            break;
                        }
                    }
                }
            }
        }
    }

    pub fn update<F>(&self, key: &Pubkey, updatefn: F)
    where
        F: Fn(Option<(&[T], u64)>) -> Option<(Vec<T>, u64)>,
    {
        let ix = self.bucket_ix(key);
        let mut bucket = self.buckets[ix].write().unwrap();
        if bucket.is_none() {
            *bucket = Some(Bucket::new(self.drives.clone()));
        }
        let bucket = bucket.as_mut().unwrap();
        let current = bucket.read_value(key);
        let new = updatefn(current);
        if new.is_none() {
            bucket.delete_key(key);
            return;
        }
        let (new, refct) = new.unwrap();
        loop {
            let rv = bucket.try_write(key, &new, refct);
            match rv {
                Err(BucketMapError::DataNoSpace(sz)) => {
                    debug!("GROWING SPACE {:?}", sz);
                    bucket.grow_data(sz);
                    continue;
                }
                Err(BucketMapError::IndexNoSpace(sz)) => {
                    debug!("GROWING INDEX {}", sz);
                    bucket.grow_index(sz);
                    continue;
                }
                Ok(()) => return,
            }
        }
    }
    pub fn bucket_ix(&self, key: &Pubkey) -> usize {
        let location = read_be_u64(key.as_ref());
        (location >> (64 - self.bits)) as usize
    }

    pub fn addref(&self, key: &Pubkey) -> Option<u64> {
        let ix = self.bucket_ix(key);
        let mut bucket = self.buckets[ix].write().unwrap();
        bucket.as_mut()?.addref(key)
    }

    pub fn unref(&self, key: &Pubkey) -> Option<u64> {
        let ix = self.bucket_ix(key);
        let mut bucket = self.buckets[ix].write().unwrap();
        bucket.as_mut()?.unref(key)
    }

    pub fn load_ref(&self, key: &Pubkey) -> Option<u64> {
        let ix = self.bucket_ix(key);
        let bucket = self.buckets[ix].read().unwrap();
        bucket.as_ref()?.load_ref(key)
    }
}

struct Bucket<T> {
    drives: Arc<Vec<PathBuf>>,
    //index
    index: DataBucket,
    //random offset for the index
    random: u64,
    //data buckets to store SlotSlice up to a power of 2 in len
    data: Vec<DataBucket>,
    _phantom: PhantomData<T>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
struct IndexEntry {
    key: Pubkey,      // can this be smaller if we have reduced the keys into buckets already?
    ref_count: u64,   // can this be smaller? Do we ever need more than 4B refcounts?
    data_bucket: u64, // usize? or smaller. do we ever expect more than 4B buckets?
    data_location: u64, // smaller? since these are variably sized, this could get tricky. well, actually accountinfo is not variable sized...
    //if the bucket doubled, the index can be recomputed
    create_bucket_capacity: u8, // TODO: what does this mean?
    num_slots: u64,             // can this be smaller? epoch size should be the max len
}

impl IndexEntry {
    fn data_loc(&self, bucket: &DataBucket) -> u64 {
        self.data_location << (bucket.capacity - self.create_bucket_capacity)
    }

    fn read_value<'a, T>(&self, bucket: &'a Bucket<T>) -> Option<(&'a [T], u64)> {
        let data_bucket = &bucket.data[self.data_bucket as usize];
        let loc = self.data_loc(data_bucket);
        let uid = Self::key_uid(&self.key);
        assert_eq!(uid, bucket.data[self.data_bucket as usize].uid(loc));
        let slice = bucket.data[self.data_bucket as usize].get_cell_slice(loc, self.num_slots);
        Some((slice, self.ref_count))
    }
    fn key_uid(key: &Pubkey) -> u64 {
        let mut s = DefaultHasher::new();
        key.hash(&mut s);
        s.finish().max(1u64)
    }
}

const MAX_SEARCH: u64 = 16;

impl<T: Clone> Bucket<T> {
    fn new(drives: Arc<Vec<PathBuf>>) -> Self {
        let index = DataBucket::new(drives.clone(), 1, std::mem::size_of::<IndexEntry>() as u64);
        Self {
            random: thread_rng().gen(),
            drives,
            index,
            data: vec![],
            _phantom: PhantomData::default(),
        }
    }

    fn keys(&self) -> Vec<Pubkey> {
        let mut rv = vec![];
        for i in 0..self.index.num_cells() {
            if self.index.uid(i) == 0 {
                continue;
            }
            let ix: &IndexEntry = self.index.get(i);
            rv.push(ix.key);
        }
        rv
    }

    fn values(&self) -> Vec<Vec<T>> {
        let mut rv = vec![];
        for i in 0..self.index.num_cells() {
            if self.index.uid(i) == 0 {
                continue;
            }
            let ix: &IndexEntry = self.index.get(i);
            let val = ix.read_value(self);
            if val.is_none() {
                continue;
            }
            rv.push(val.unwrap().0.to_vec());
        }
        rv
    }

    fn find_entry(&self, key: &Pubkey) -> Option<(&IndexEntry, u64)> {
        Self::bucket_find_entry(&self.index, key, self.random)
    }

    fn find_entry_mut(&self, key: &Pubkey) -> Option<(&mut IndexEntry, u64)> {
        Self::bucket_find_entry_mut(&self.index, key, self.random)
    }

    fn bucket_find_entry_mut<'a>(
        index: &'a DataBucket,
        key: &Pubkey,
        random: u64,
    ) -> Option<(&'a mut IndexEntry, u64)> {
        let ix = Self::bucket_index_ix(index, key, random);
        for i in ix..ix + MAX_SEARCH {
            let ii = i % index.num_cells();
            if index.uid(ii) == 0 {
                continue;
            }
            let elem: &mut IndexEntry = index.get_mut(ii);
            if elem.key == *key {
                return Some((elem, ii));
            }
        }
        None
    }

    fn bucket_find_entry<'a>(
        index: &'a DataBucket,
        key: &Pubkey,
        random: u64,
    ) -> Option<(&'a IndexEntry, u64)> {
        let ix = Self::bucket_index_ix(index, key, random);
        for i in ix..ix + MAX_SEARCH {
            let ii = i % index.num_cells();
            if index.uid(ii) == 0 {
                continue;
            }
            let elem: &IndexEntry = index.get(ii);
            if elem.key == *key {
                return Some((elem, ii));
            }
        }
        None
    }

    fn bucket_create_key(
        index: &DataBucket,
        key: &Pubkey,
        elem_uid: u64,
        random: u64,
        ref_count: u64,
    ) -> Result<u64, BucketMapError> {
        let ix = Self::bucket_index_ix(index, key, random);
        for i in ix..ix + MAX_SEARCH {
            let ii = i as u64 % index.num_cells();
            if index.uid(ii) != 0 {
                continue;
            }
            index.allocate(ii, elem_uid).unwrap();
            let mut elem: &mut IndexEntry = index.get_mut(ii);
            elem.key = *key;
            elem.ref_count = ref_count;
            elem.data_bucket = 0;
            elem.data_location = 0;
            elem.create_bucket_capacity = 0;
            elem.num_slots = 0;
            debug!(
                "INDEX ALLOC {:?} {} {} {}",
                key, ii, index.capacity, elem_uid
            );
            return Ok(ii);
        }
        Err(BucketMapError::IndexNoSpace(index.capacity))
    }

    fn addref(&mut self, key: &Pubkey) -> Option<u64> {
        let (elem, _) = self.find_entry_mut(key)?;
        elem.ref_count += 1;
        Some(elem.ref_count)
    }

    fn unref(&mut self, key: &Pubkey) -> Option<u64> {
        let (elem, _) = self.find_entry_mut(key)?;
        elem.ref_count -= 1;
        Some(elem.ref_count)
    }

    fn load_ref(&self, key: &Pubkey) -> Option<u64> {
        let (elem, _) = self.find_entry(key)?;
        Some(elem.ref_count)
    }

    fn create_key(&self, key: &Pubkey, ref_count: u64) -> Result<u64, BucketMapError> {
        Self::bucket_create_key(&self.index, key, IndexEntry::key_uid(key), self.random, ref_count)
    }

    pub fn read_value(&self, key: &Pubkey) -> Option<(&[T], u64)> {
        debug!("READ_VALUE: {:?}", key);
        let (elem, _) = self.find_entry(key)?;
        elem.read_value(self)
    }

    fn try_write(&mut self, key: &Pubkey, data: &[T], ref_count: u64) -> Result<(), BucketMapError> {
        let mut index_entry = self.find_entry_mut(key);
        let (elem, elem_ix) = if index_entry.is_none() {
            let ii = self.create_key(key, ref_count)?;
            let elem = self.index.get_mut(ii); // get_mut here is right?
            (elem, ii)
        } else {
            let res = index_entry.unwrap();
            if ref_count != res.0.ref_count {
                res.0.ref_count = ref_count;
            }
            res
        };
        let elem_uid = self.index.uid(elem_ix);
        let best_fit_bucket = Self::best_fit(data.len() as u64);
        if self.data.get(best_fit_bucket as usize).is_none() {
            return Err(BucketMapError::DataNoSpace((best_fit_bucket, 0)));
        }
        let current_bucket = &self.data[elem.data_bucket as usize];
        if best_fit_bucket == elem.data_bucket && elem.num_slots > 0 {
            //in place update
            let elem_loc = elem.data_loc(current_bucket);
            let slice: &mut [T] = current_bucket.get_mut_cell_slice(elem_loc, data.len() as u64);
            //let elem: &mut IndexEntry = self.index.get_mut(elem_ix);
            assert!(current_bucket.uid(elem_loc) == elem_uid);
            elem.num_slots = data.len() as u64;
            slice.clone_from_slice(data);
            return Ok(());
        } else {
            //need to move the allocation to a best fit spot
            let best_bucket = &self.data[best_fit_bucket as usize];
            let cap_power = best_bucket.capacity;
            let cap = best_bucket.num_cells();
            let pos = thread_rng().gen_range(0, cap);
            for i in pos..pos + MAX_SEARCH as u64 {
                let ix = i % cap;
                if best_bucket.uid(ix) == 0 {
                    let elem_loc = elem.data_loc(current_bucket);
                    if elem.num_slots > 0 {
                        current_bucket.free(elem_loc, elem_uid).unwrap();
                    }
                    // elem: &mut IndexEntry = self.index.get_mut(elem_ix);
                    elem.data_bucket = best_fit_bucket;
                    elem.data_location = ix;
                    elem.create_bucket_capacity = best_bucket.capacity;
                    elem.num_slots = data.len() as u64;
                    debug!(
                        "DATA ALLOC {:?} {} {} {}",
                        key, elem.data_location, best_bucket.capacity, elem_uid
                    );
                    if elem.num_slots > 0 {
                        best_bucket.allocate(ix, elem_uid).unwrap();
                        let slice = best_bucket.get_mut_cell_slice(ix, data.len() as u64);
                        slice.clone_from_slice(data);
                    }
                    return Ok(());
                }
            }
            Err(BucketMapError::DataNoSpace((best_fit_bucket, cap_power)))
        }
    }

    fn delete_key(&mut self, key: &Pubkey) {
        if let Some((elem, elem_ix)) = self.find_entry(key) {
            let elem_uid = self.index.uid(elem_ix);
            if elem.num_slots > 0 {
                let data_bucket = &self.data[elem.data_bucket as usize];
                let loc = elem.data_loc(data_bucket);
                debug!(
                    "DATA FREE {:?} {} {} {}",
                    key, elem.data_location, data_bucket.capacity, elem_uid
                );
                data_bucket.free(loc, elem_uid).unwrap();
            }
            debug!("INDEX FREE {:?} {}", key, elem_uid);
            self.index.free(elem_ix, elem_uid).unwrap();
        }
    }

    fn grow_index(&mut self, sz: u8) {
        if self.index.capacity == sz {
            debug!("GROW_INDEX: {}", sz);
            for i in 1.. {
                //increasing the capacity by ^4 reduces the
                //likelyhood of a re-index collision of 2^(max_search)^2
                //1 in 2^32
                let index = DataBucket::new_with_capacity(
                    self.drives.clone(),
                    1,
                    std::mem::size_of::<IndexEntry>() as u64,
                    self.index.capacity + i * 2,
                );
                let random = thread_rng().gen();
                let rvs: Vec<bool> = (0..self.index.num_cells())
                    .into_iter()
                    .map(|ix| {
                        if 0 != self.index.uid(ix) {
                            let elem: &IndexEntry = self.index.get(ix);
                            let uid = self.index.uid(ix);
                            let ref_count = 0; // ??? TODO
                            let new_ix = Self::bucket_create_key(&index, &elem.key, uid, random, ref_count);
                            if new_ix.is_err() {
                                return false;
                            }
                            let new_ix = new_ix.unwrap();
                            let new_elem: &mut IndexEntry = index.get_mut(new_ix);
                            *new_elem = *elem;
                            let dbg_elem: IndexEntry = *new_elem;
                            assert_eq!(
                                Self::bucket_find_entry(&index, &elem.key, random).unwrap(),
                                (&dbg_elem, new_ix)
                            );
                        }
                        true
                    })
                    .collect();
                if rvs.into_iter().all(|x| x) {
                    self.index = index;
                    self.random = random;
                    break;
                }
            }
        }
    }

    fn best_fit(sz: u64) -> u64 {
        (sz as f64).log2().ceil() as u64
    }

    fn grow_data(&mut self, sz: (u64, u8)) {
        if self.data.get(sz.0 as usize).is_none() {
            for i in self.data.len() as u64..(sz.0 + 1) {
                self.data.push(DataBucket::new(
                    self.drives.clone(),
                    1 << i,
                    std::mem::size_of::<T>() as u64,
                ))
            }
        }
        if self.data[sz.0 as usize].capacity == sz.1 {
            debug!("GROW_DATA: {} {}", sz.0, sz.1);
            self.data[sz.0 as usize].grow();
        }
    }

    fn bucket_index_ix(index: &DataBucket, key: &Pubkey, random: u64) -> u64 {
        let uid = IndexEntry::key_uid(key);
        let mut s = DefaultHasher::new();
        uid.hash(&mut s);
        //the locally generated random will make it hard for an attacker
        //to deterministically cause all the pubkeys to land in the same
        //location in any bucket on all validators
        random.hash(&mut s);
        let ix = s.finish();
        let location = ix % index.num_cells();
        debug!(
            "INDEX_IX: {:?} uid:{} loc: {} cap:{}",
            key,
            uid,
            location,
            index.num_cells()
        );
        location
    }
}

fn read_be_u64(input: &[u8]) -> u64 {
    assert!(input.len() >= std::mem::size_of::<u64>());
    u64::from_be_bytes(input[0..std::mem::size_of::<u64>()].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_map_test_insert() {
        let key = Pubkey::new_unique();
        let tmpdir = std::env::temp_dir().join("bucket_map_test_insert");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(1, drives);
        index.update(&key, |_| Some(vec![0]));
        assert_eq!(index.read_value(&key), Some(vec![0]));
        std::fs::remove_dir_all(tmpdir).unwrap();
    }

    #[test]
    fn bucket_map_test_update() {
        let key = Pubkey::new_unique();
        let tmpdir = std::env::temp_dir().join("bucket_map_test_update");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(1, drives);
        index.update(&key, |_| Some(vec![0]));
        assert_eq!(index.read_value(&key), Some(vec![0]));
        index.update(&key, |_| Some(vec![1]));
        assert_eq!(index.read_value(&key), Some(vec![1]));
        std::fs::remove_dir_all(tmpdir).unwrap();
    }

    #[test]
    fn bucket_map_test_update_to_0_len() {
        solana_logger::setup();
        let key = Pubkey::new_unique();
        let tmpdir = std::env::temp_dir().join("bucket_map_test_update");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(1, drives);
        index.update(&key, |_| Some((vec![0], 1)));
        assert_eq!(index.read_value(&key), Some((vec![0], 1)));
        // sets len to 0, updates in place
        index.update(&key, |_| Some((vec![], 1)));
        assert_eq!(index.read_value(&key), Some((vec![], 1)));
        // sets len to 0, doesn't update in place - finds a new place, which causes us to no longer have an allocation in data
        index.update(&key, |_| Some((vec![], 2)));
        assert_eq!(index.read_value(&key), Some((vec![], 2)));
        // sets len to 1, doesn't update in place - finds a new place
        index.update(&key, |_| Some((vec![1], 2)));
        assert_eq!(index.read_value(&key), Some((vec![1], 2)));
        std::fs::remove_dir_all(tmpdir).unwrap();
    }

    #[test]
    fn bucket_map_test_delete() {
        let tmpdir = std::env::temp_dir().join("bucket_map_test_delete");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(1, drives);
        for i in 0..10 {
            let key = Pubkey::new_unique();
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some(vec![i]));
            assert_eq!(index.read_value(&key), Some(vec![i]));

            index.delete_key(&key);
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some(vec![i]));
            assert_eq!(index.read_value(&key), Some(vec![i]));
            index.delete_key(&key);
        }
        std::fs::remove_dir_all(tmpdir).unwrap();
    }

    #[test]
    fn bucket_map_test_delete_2() {
        let tmpdir = std::env::temp_dir().join("bucket_map_test_delete_2");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(2, drives);
        for i in 0..100 {
            let key = Pubkey::new_unique();
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some(vec![i]));
            assert_eq!(index.read_value(&key), Some(vec![i]));

            index.delete_key(&key);
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some(vec![i]));
            assert_eq!(index.read_value(&key), Some(vec![i]));
            index.delete_key(&key);
        }
        std::fs::remove_dir_all(tmpdir).unwrap();
    }

    #[test]
    fn bucket_map_test_n_drives() {
        let tmpdir = std::env::temp_dir().join("bucket_map_test_n_drives");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(2, drives);
        for i in 0..100 {
            let key = Pubkey::new_unique();
            index.update(&key, |_| Some(vec![i]));
            assert_eq!(index.read_value(&key), Some(vec![i]));
        }
        std::fs::remove_dir_all(tmpdir).unwrap();
    }
    #[test]
    fn bucket_map_test_grow_read() {
        let tmpdir = std::env::temp_dir().join("bucket_map_test_grow_read");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(2, drives);
        let keys: Vec<Pubkey> = (0..100).into_iter().map(|_| Pubkey::new_unique()).collect();
        for k in 0..keys.len() {
            let key = &keys[k];
            let i = read_be_u64(key.as_ref());
            index.update(&key, |_| Some(vec![i]));
            assert_eq!(index.read_value(&key), Some(vec![i]));
            for j in 0..k {
                let key = &keys[j];
                let i = read_be_u64(key.as_ref());
                debug!("READ: {:?} {}", key, i);
                assert_eq!(index.read_value(&key), Some(vec![i]));
            }
        }
        std::fs::remove_dir_all(tmpdir).unwrap();
    }

    #[test]
    fn bucket_map_test_n_delete() {
        let tmpdir = std::env::temp_dir().join("bucket_map_test_n_delete");
        std::fs::create_dir_all(tmpdir.clone()).unwrap();
        let drives = Arc::new(vec![tmpdir.clone()]);
        let index = BucketMap::new(2, drives);
        let keys: Vec<Pubkey> = (0..20).into_iter().map(|_| Pubkey::new_unique()).collect();
        for key in keys.iter() {
            let i = read_be_u64(key.as_ref());
            index.update(&key, |_| Some(vec![i]));
            assert_eq!(index.read_value(&key), Some(vec![i]));
        }
        for key in keys.iter() {
            let i = read_be_u64(key.as_ref());
            debug!("READ: {:?} {}", key, i);
            assert_eq!(index.read_value(&key), Some(vec![i]));
        }
        for k in 0..keys.len() {
            let key = &keys[k];
            index.delete_key(&key);
            assert_eq!(index.read_value(&key), None);
            for k in (k + 1)..keys.len() {
                let key = &keys[k];
                let i = read_be_u64(key.as_ref());
                assert_eq!(index.read_value(&key), Some(vec![i]));
            }
        }
        std::fs::remove_dir_all(tmpdir).unwrap();
    }
}
