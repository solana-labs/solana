//! BucketMap is a mostly contention free concurrent map backed by MmapMut

use crate::accounts_db::AccountInfo;
use crate::data_bucket::DataBucket;
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
    DataNoSpace((usize, u64)),
    IndexNoSpace(u64),
}

impl BucketMap {
    pub fn read_value(&self, pubkey: &Pubkey) -> Option<Vec<SlotInfo>> {
        let ix = self.bucket_ix(pubkey);
        self.buckets[ix].read().unwrap().as_ref().and_then(|x| x.read_value(pubkey))
    }

    pub fn delete_key(&self, pubkey: &Pubkey) {
        let ix = self.bucket_ix(pubkey);
        let bucket = self.buckets[ix].read().unwrap();
        if bucket.is_none() {
            bucket.as_ref().unwrap().delete_key(pubkey);
        }
    }

    //updatefn must be re-entrant
    pub fn update(
        &self,
        pubkey: &Pubkey,
        updatefn: fn(Option<Vec<SlotInfo>>) -> Vec<SlotInfo>,
    ) -> Result<(), BucketMapError> {
        let ix = self.bucket_ix(pubkey);
        let mut bucket = self.buckets[ix].write().unwrap();
        if bucket.is_none() {
            *bucket  = Some(Bucket::new(&self.drives));
        }
        let bucket = bucket.as_mut().unwrap();
        let current = bucket.read_value(pubkey);
        let new = updatefn(current);
        loop {
            let rv = bucket.try_write(pubkey, current.is_none(), &new);
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

const MAX_SEARCH: usize = 64;

impl Bucket {
    fn new(drives: Arc<Vec<PathBuf>>) -> Self {
        let index = DataBucket::new(drives, std::mem::size_of::<IndexEntry>());
        Self {
            drives
            index,
            data: vec![],
        }
        unimplemented!();
    }
    fn read_value(&self, key: &Pubkey) -> Option<Vec<SlotInfo>> {
        let ix = self.index_ix(key);
        for i in ix .. ix + MAX_SEARCH {
            let ii = i % self.index.capacity;
            if self.index.uid(ii) == 0 {
                continue;
            }
            let elem: &IndexEntry = self.index.get(ii);
            if elem.key == key {
                let cur_cap = self.data[eleme.data_bucket].capacity;
                let loc = elem.data_location << cur_cap - elem.create_bucket_capacity;
                let slice = self.data[eleme.data_bucket].get(loc);
                return Some(slice);
            }
        }
        None
    }
    fn allocate(&mut self, pubkey: &Pubkey, new: bool, data: &SlotSlice) -> Result<(), BucketMapError> {
        if new {
        }
        for i in ix .. ix + MAX_SEARCH {
        }
    }

    fn try_write(&mut self, pubkey: &Pubkey, new: bool, data: &SlotSlice) -> Result<(), BucketMapError> {
        if new {
        }
        for i in ix .. ix + MAX_SEARCH {
        }
    }
    fn delete_key(&self, _key: &Pubkey) {}
    fn grow_index(&mut self, sz: u64) {
        if self.index.capacity != sz {
            self.index.grow();
        }
    }
    fn grow_data(&mut self, sz: (usize, u64)) {
        if self.data.get(sz.0).is_none() {
            for i in self.data.len() .. (sz.0 + 1) {
                self.data.push(DataBucket::new(self.drives, 1<<i))
            }
        }
        if self.data[sz.0].capacity == sz.1 {
            self.data[sz.0].grow();
        }
    }
    fn index_ix(&self, pubkey: &Pubkey) -> usize {
        let location = read_be_u64(pubkey.as_ref());
        location % self.index.capacity
    }
}

pub type SlotInfo = (Slot, AccountInfo);

pub type SlotSlice = [SlotInfo];

fn read_be_u64(input: &[u8]) -> u64 {
    u64::from_be_bytes(input[0..std::mem::size_of::<u64>()].try_into().unwrap())
}
