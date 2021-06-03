//! BucketMap is a mostly contention free concurrent map backed by MmapMut

use crate::accounts_db::AccountInfo;
use crate::data_bucket::DataBucket;
use std::path::PathBuf;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use core::sync::atomic::AtomicBool;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::slot_history::Slot;
use std::collections::HashMap;
use std::convert::TryInto;
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
        let ix = self.bucket_ix(pubkey);
        self.check_lock(pubkey);
        let spinlock = self.key_locks.read().unwrap().get(pubkey).unwrap();
        spinlock.lock();
        let rv = self.masks[ix].bucket.read(pubkey);
        spinlock.unlock();
        rv
    }

    pub fn delete(&self, pubkey: &Pubkey) {
        let ix = self.bucket_ix(pubkey);
        self.check_lock(pubkey);
        let spinlock = self.key_locks.read().unwrap().get(pubkey).unwrap();
        spinlock.lock();
        self.masks[ix].bucket.delete(pubkey);
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
                let ix = self.bucket_ix(pubkey);
                let mut masks = self.masks.write();
                let new_bucket = masks[ix].split(ix);
                let mut new_masks = Vec::new();
                for i in 0..masks.len() {
                    new_masks[i] = masks[i];
                    new_masks[i + 1] = masks[i];
                    if i as u64 == ix {
                        new_masks[i + 1] = new_bucket;
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
            let rmasks = self.masks.read().unwrap();
            let ix = self.bucket_ix(pubkey, &rmasks);
            let current = rmasks[ix].bucket.read(pubkey);
            if current.is_none() {
                self.new_key_locked(pubkey);
            }
            let new = updatefn(current);
            rv = rmasks[ix].bucket.read().unrwap().try_write(pubkey, new);
            if let Err(BucketMapError::DataNoSpace(ix)) = rv {
                drop(rmasks)
                self.masks[ix].bucket.write().unwrap().grow(new.len());
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
            key_locks.insert(pubkey, Arc::new(SpinLock::new()));
        }
    }

    fn bucket_ix(&self, pubkey: &Pubkey, &[Arc<RwLock<Bucket>>>]) -> u64 {
        let location = read_be_u64(pubkey.as_ref());
        let bits = (masks.len() as f64).log2() as u64;
        location >> (64 - bits)
    }
}

struct Bucket {
    version: AtomicU64,
    //index
    index: DataBucket,
    //data buckets to store SlotSlice up to a power of 2 in len
    data: Vec<Arc<DataBucket>>,
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

enum BucketMapError {
    DataNoSpace(u64),
    IndexNoSpace,
}

pub type SlotSlice = [SlotInfo];

fn read_be_u64(input: &[u8]) -> u64 {
    u64::from_be_bytes(input[0..std::mem::size_of::<u64>()].try_into().unwrap())
}
