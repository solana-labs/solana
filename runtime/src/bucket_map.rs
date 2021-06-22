//! BucketMap is a mostly contention free concurrent map backed by MmapMut

use crate::accounts_db::AccountInfo;
use crate::data_bucket::DataBucket;
use rand::thread_rng;
use rand::Rng;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::slot_history::Slot;
use std::convert::TryInto;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

pub struct BucketMap {
    // power of 2 size masks point to the same bucket
    buckets: Vec<RwLock<Option<Bucket>>>,
    drives: Arc<Vec<PathBuf>>,
}

pub enum BucketMapError {
    DataNoSpace((u64, u64)),
    IndexNoSpace(u64),
}

impl BucketMap {
    pub fn read_value(&self, pubkey: &Pubkey) -> Option<Vec<SlotInfo>> {
        let ix = self.bucket_ix(pubkey);
        self.buckets[ix].read().unwrap().as_ref().and_then(|x| {
            let slice = x.read_value(pubkey)?;
            let mut vec = Vec::with_capacity(slice.len());
            vec.extend_from_slice(slice);
            Some(vec)
        })
    }

    pub fn delete_key(&self, pubkey: &Pubkey) {
        let ix = self.bucket_ix(pubkey);
        let mut bucket = self.buckets[ix].write().unwrap();
        if !bucket.is_none() {
            bucket.as_mut().unwrap().delete_key(pubkey);
        }
    }

    //updatefn must be re-entrant
    pub fn update(
        &self,
        pubkey: &Pubkey,
        updatefn: fn(Option<&[SlotInfo]>) -> Option<&[SlotInfo]>,
    ) -> Result<(), BucketMapError> {
        let ix = self.bucket_ix(pubkey);
        let mut bucket = self.buckets[ix].write().unwrap();
        if bucket.is_none() {
            *bucket = Some(Bucket::new(self.drives.clone()));
        }
        let bucket = bucket.as_mut().unwrap();
        let current = bucket.read_value(pubkey);
        let new = updatefn(current);
        if new.is_none() {
            bucket.delete_key(pubkey);
            return Ok(());
        }
        let new = new.unwrap();
        loop {
            let rv = bucket.try_write(pubkey, &new);
            if let Err(BucketMapError::DataNoSpace(sz)) = rv {
                bucket.grow_data(sz);
                continue;
            } else if let Err(BucketMapError::IndexNoSpace(sz)) = rv {
                bucket.grow_index(sz);
                continue;
            }
            return rv;
        }
    }
    fn bucket_ix(&self, pubkey: &Pubkey) -> usize {
        let location = read_be_u64(pubkey.as_ref());
        let bits = (self.buckets.len() as f64).log2().ceil() as u64;
        (location >> (64 - bits)) as usize
    }
}

struct Bucket {
    version: u64,
    drives: Arc<Vec<PathBuf>>,
    //index
    index: DataBucket,
    //data buckets to store SlotSlice up to a power of 2 in len
    data: Vec<DataBucket>,
}

#[repr(C)]
struct IndexEntry {
    key: Pubkey,
    data_bucket: u64,
    data_location: u64,
    //if the bucket doubled, the index can be recomputed
    create_bucket_capacity: u64,
    num_slots: u64,
}

impl IndexEntry {
    fn data_loc(&self, elem_cap: u64) -> u64 {
        self.data_location << elem_cap as u64 - self.create_bucket_capacity
    }
}

const MAX_SEARCH: u64 = 16;
const MIN_ELEMS: u64 = 16;

impl Bucket {
    fn new(drives: Arc<Vec<PathBuf>>) -> Self {
        let index = DataBucket::new(drives, 1, std::mem::size_of::<IndexEntry>() as u64);
        Self {
            version: 0,
            drives,
            index,
            data: vec![],
        }
    }

    fn find_index(&self, key: &Pubkey) -> Option<(&IndexEntry, u64)> {
        let ix = self.index_ix(key);
        for i in ix..ix + MAX_SEARCH {
            let ii = i % self.index.capacity;
            if self.index.uid(ii) == 0 {
                continue;
            }
            let elem: &IndexEntry = self.index.get(ii);
            if elem.key == *key {
                return Some((elem, ii));
            }
        }
        None
    }

    fn create_key(&mut self, key: &Pubkey) -> Result<(&IndexEntry, u64), BucketMapError> {
        let ix = self.index_ix(key);
        for i in ix..ix + MAX_SEARCH {
            let ii = i as u64 % self.index.capacity;
            if self.index.uid(ii) != 0 {
                continue;
            }
            self.index.allocate(ii, self.version);
            self.version += 1;
            let mut elem: &mut IndexEntry = self.index.get_mut(ii);
            elem.key = *key;
            elem.data_bucket = 0;
            elem.data_location = 0;
            elem.create_bucket_capacity = 0;
            elem.num_slots = 0;
            return Ok((elem, ii));
        }
        Err(BucketMapError::IndexNoSpace(self.index.capacity))
    }

    pub fn read_value(&self, key: &Pubkey) -> Option<&[SlotInfo]> {
        let (elem, _) = self.find_index(key)?;
        let data_bucket = &self.data[elem.data_bucket as usize];
        let cur_cap = data_bucket.capacity;
        let loc = elem.data_location << cur_cap - elem.create_bucket_capacity as u64;
        let slice = self.data[elem.data_bucket as usize].get_cell_slice(loc, elem.num_slots);
        Some(slice)
    }

    fn try_write(&mut self, key: &Pubkey, data: &SlotSlice) -> Result<(), BucketMapError> {
        let index_entry = self.find_index(key);
        let (elem, elem_ix) = if index_entry.is_none() {
            self.create_key(key)?
        } else {
            index_entry.unwrap()
        };
        let elem_uid = self.index.uid(elem_ix);
        let cur_cap = self.data[elem.data_bucket as usize].capacity;
        let elem_loc = elem.data_location << cur_cap - elem.create_bucket_capacity;
        let best_fit_bucket = Self::best_fit(data.len() as u64);
        if self.data.get(best_fit_bucket as usize).is_none() {
            return Err(BucketMapError::DataNoSpace((best_fit_bucket, 0)));
        }
        let current_bucket = self.data[elem.data_bucket as usize];
        if best_fit_bucket == elem.data_bucket {
            //in place update
            let slice: &mut [SlotInfo] =
                current_bucket.get_mut_cell_slice(elem_loc, data.len() as u64);
            slice.clone_from_slice(data);
            return Ok(());
        } else {
            //need to move the allocation to a best fit spot
            let best_bucket = &self.data[best_fit_bucket as usize];
            let cap = best_bucket.capacity;
            let pos = thread_rng().gen_range(0, cap);
            for i in pos..pos + MAX_SEARCH as u64 {
                let ix = i % cap;
                if best_bucket.uid(ix) == 0 {
                    current_bucket.free(elem_loc, elem_uid);
                    best_bucket.allocate(ix, elem_uid);
                    let slice = best_bucket.get_mut_cell_slice(ix, data.len() as u64);
                    elem.data_bucket = best_fit_bucket;
                    elem.data_location = ix;
                    elem.create_bucket_capacity = best_bucket.capacity;
                    elem.num_slots = data.len() as u64;
                    slice.clone_from_slice(data);
                    return Ok(());
                }
            }
            Err(BucketMapError::DataNoSpace((best_fit_bucket, cap)))
        }
    }

    fn delete_key(&mut self, key: &Pubkey) {
        if let Some((elem, elem_ix)) = self.find_index(key) {
            let elem_uid = self.index.uid(elem_ix);
            if elem.num_slots > 0 {
                let data_bucket = self.data[elem.data_bucket as usize];
                let loc = elem.data_loc(data_bucket.capacity);
                data_bucket.free(loc, elem_uid);
            }
            self.index.free(elem_ix, elem_uid);
        }
    }

    fn grow_index(&mut self, sz: u64) {
        if self.index.capacity != sz {
            self.index.grow();
        }
    }

    fn best_fit(sz: u64) -> u64 {
        ((sz.max(MIN_ELEMS) - MIN_ELEMS) as f64).log2().ceil() as u64
    }

    fn grow_data(&mut self, sz: (u64, u64)) {
        if self.data.get(sz.0 as usize).is_none() {
            for i in self.data.len() as u64..(sz.0 + 1) {
                self.data.push(DataBucket::new(
                    self.drives,
                    (1 << i) + MIN_ELEMS,
                    std::mem::size_of::<SlotInfo>() as u64,
                ))
            }
        }
        if self.data[sz.0 as usize].capacity == sz.1 {
            self.data[sz.0 as usize].grow();
        }
    }
    fn index_ix(&self, pubkey: &Pubkey) -> u64 {
        let location = read_be_u64(pubkey.as_ref());
        location % self.index.capacity
    }
}

pub type SlotInfo = (Slot, AccountInfo);

pub type SlotSlice = [SlotInfo];

fn read_be_u64(input: &[u8]) -> u64 {
    u64::from_be_bytes(input[0..std::mem::size_of::<u64>()].try_into().unwrap())
}
