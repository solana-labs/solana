//! BucketMap is a mostly contention free concurrent map backed by MmapMut

use crate::accounts_db::AccountInfo;
use crate::data_bucket::DataBucket;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::slot_history::Slot;
use std::collections::HashMap;
use std::convert::TryInto;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

pub struct BucketMap {
    // power of 2 size masks point to the same bucket
    masks: RwLock<Vec<Arc<RwLock<Bucket>>>>,
    drives: Arc<Vec<PathBuf>>,
    key_locks: RwLock<HashMap<Pubkey, Arc<SpinLock>>>,
}

struct SpinLock {
    lock: AtomicBool,
}

impl SpinLock {
    fn new() -> Self {
        SpinLock {
            lock: AtomicBool::new(false),
        }
    }

    fn lock(&self) {
        while Ok(false)
            != self
                .lock
                .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        {}
    }
    fn unlock(&self) {
        self.lock.store(false, Ordering::Relaxed)
    }
}

enum BucketMapError {
    DataNoSpace(u64),
    IndexNoSpace,
}

impl BucketMap {
    pub fn scan<T>(&self, accum: &mut T, scanfn: fn(&Bucket, accum: &mut T)) {
        let masks = self.masks.write().unwrap();
        for i in 0..masks.len() {
            if i > 1 && masks[i].ptr_eq(masks[i - 1]) {
                continue;
            }
            scanfn(&masks[i].read().unwrap(), accum)
        }
    }

    pub fn read(&self, pubkey: &Pubkey) -> Option<&SlotSlice> {
        let ix = Self::bucket_ix(pubkey);
        self.check_lock(pubkey);
        let spinlock = self.key_locks.read().unwrap().get(pubkey).unwrap();
        spinlock.lock();
        let rv = self.masks[ix].bucket.read(pubkey);
        spinlock.unlock();
        rv
    }

    pub fn delete(&self, pubkey: &Pubkey) {
        self.check_lock(pubkey);
        let spinlock = self.key_locks.read().unwrap().get(pubkey).unwrap();
        spinlock.lock();
        let masks = self.masks[ix].read().unwrap();
        let ix = Self::bucket_ix(pubkey, masks.len());
        masks..delete(pubkey);
        spinlock.unlock();
    }

    //updatefn must be re-entrant
    pub fn update(
        &self,
        pubkey: &Pubkey,
        updatefn: fn(Option<&SlotSlice>) -> &SlotSlice,
    ) -> Result<(), BucketMapError> {
        loop {
            let e = self.try_update(pubkey, updatefn);
            if let Err(BucketMapError::IndexNoSpace) = e {
                let mut masks = self.masks.write().unwrap();
                let ix = Self::bucket_ix(pubkey, masks.len());
                let new_bucket = masks[ix].write().unwrap().split(ix);
                let mut new_masks = Vec::new();
                for i in 0..masks.len() {
                    new_masks.push(masks[i].clone());
                    new_masks.push(masks[i].clone());
                    if i == ix {
                        new_masks[i * 2 + 1] = Arc::new(RwLock::new(new_bucket));
                    }
                }
                *masks = new_masks;
                continue;
            }
            return e;
        }
    }

    fn try_update(
        &self,
        pubkey: &Pubkey,
        updatefn: fn(Option<&SlotSlice>) -> &SlotSlice,
    ) -> Result<(), BucketMapError> {
        self.check_lock(pubkey);
        let spinlock = self.key_locks.read().unwrap().get(pubkey).unwrap();

        let mut rv = Ok(());
        spinlock.lock();
        loop {
            let masks = self.masks.read().unwrap();
            let ix = Self::bucket_ix(pubkey, masks.len());
            let current = masks[ix].read().unwrap().read_value(pubkey);
            if current.is_none() {
                masks[ix].read().unwrap().new_key(pubkey);
            }
            let new = updatefn(current);
            rv = masks[ix].read().unwrap().try_write(pubkey, new);
            if let Err(BucketMapError::DataNoSpace(ix)) = rv {
                //unlock the read lock
                drop(masks);
                let masks = self.masks.write().unwrap();
                //ix is invalidated once we drop the lock
                let ix = Self::bucket_ix(pubkey, masks.len());
                masks[ix].write().unwrap().grow(new.len());
                continue;
            }
            break;
        }
        spinlock.unlock();
        return rv;
    }

    fn check_lock(&self, pubkey: &Pubkey) {
        let key_locks = self.key_locks.read().unwrap();
        if key_locks.get(pubkey).is_none() {
            drop(key_locks);
            let key_locks = self.key_locks.write().unwrap();
            key_locks.insert(*pubkey, Arc::new(SpinLock::new()));
        }
    }

    fn bucket_ix(pubkey: &Pubkey, masks_len: usize) -> usize {
        let location = read_be_u64(pubkey.as_ref());
        let bits = (masks_len as f64).log2() as u64;
        (location >> (64 - bits)) as usize
    }
}

struct Bucket {
    version: AtomicU64,
    //index
    index: DataBucket,
    //data buckets to store SlotSlice up to a power of 2 in len
    data: Vec<Arc<DataBucket>>,
}

impl Bucket {
    fn read_value(&mut self, key: &Pubkey) -> Option<&SlotSlice> {
        None
    }
    fn new_key(&mut self, key: &Pubkey) {}
    fn grow(&mut self, size: usize) {}
    fn split(&mut self, cur_ix: usize) -> Self {
       unimplemented!(); 
    }
    fn try_write(&mut self, pubkey: &Pubkey, data: &SlotSlice) -> Result<(), BucketMapError> {
        Ok(())
    }
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

pub type SlotInfo = (Slot, AccountInfo);

pub type SlotSlice = [SlotInfo];

fn read_be_u64(input: &[u8]) -> u64 {
    u64::from_be_bytes(input[0..std::mem::size_of::<u64>()].try_into().unwrap())
}
