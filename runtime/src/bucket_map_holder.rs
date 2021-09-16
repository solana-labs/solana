use crate::accounts_index::{IndexValue, SlotList};
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::waitable_condvar::WaitableCondvar;
use solana_sdk::pubkey::Pubkey;
use std::collections::{hash_map::Entry, HashMap};
use std::fmt::Debug;
use std::ops::RangeBounds;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
type RefCount = u64;

// will eventually hold the bucket map
pub struct BucketMapHolder<T: IndexValue> {
    pub stats: BucketMapHolderStats,
    pub disk: RwLock<HashMap<Pubkey, (SlotList<T>, RefCount)>>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: IndexValue> Debug for BucketMapHolder<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}
pub struct BucketMapKeyValue<T> {
    pub pubkey: Pubkey,
    pub ref_count: RefCount,
    pub slot_list: Vec<T>,
}

impl<T: IndexValue> BucketMapHolder<T> {
    pub fn new(_bins: usize) -> Self {
        Self {
            stats: BucketMapHolderStats::default(),
            _phantom: std::marker::PhantomData::<T>::default(),
            disk: RwLock::default(),
        }
    }
    // intended to execute in a bg thread
    pub fn background(&self, exit: Arc<AtomicBool>, wait: Arc<WaitableCondvar>) {
        loop {
            wait.wait_timeout(Duration::from_millis(10000)); // account index stats every 10 s
            if exit.load(Ordering::Relaxed) {
                break;
            }
            self.stats.report_stats();
        }
    }

    // shims for the moment for disk buckets
    pub fn keys(&self, _ix: usize) -> Option<Vec<Pubkey>> {
        let map = self.disk.read().unwrap();
        let mut r = vec![];
        for (k, _v) in map.iter() {
            r.push(*k);
        }
        Some(r)
    }

    pub fn range<R>(
        &self,
        _ix: usize,
        range: Option<&R>,
    ) -> Option<Vec<BucketMapKeyValue<(solana_sdk::clock::Slot, T)>>>
    where
        R: RangeBounds<Pubkey>,
    {
        let map = self.disk.read().unwrap();
        let mut r = vec![];
        for (k, v) in map.iter() {
            if range.map(|range| range.contains(k)).unwrap_or(true) {
                r.push(BucketMapKeyValue {
                    pubkey: *k,
                    slot_list: v.0.clone(),
                    ref_count: v.1,
                });
            }
        }
        Some(r)
    }

    pub fn read_value(&self, key: &Pubkey) -> Option<(SlotList<T>, RefCount)> {
        let map = self.disk.read().unwrap();
        map.get(key).cloned()
    }

    pub fn delete_key(&self, key: &Pubkey) {
        let mut map = self.disk.write().unwrap();
        map.remove(key);
    }

    pub fn update<F>(&self, key: &Pubkey, updatefn: F)
    where
        F: Fn(Option<(&SlotList<T>, RefCount)>) -> Option<(SlotList<T>, RefCount)>,
    {
        let mut map = self.disk.write().unwrap();
        match map.entry(*key) {
            Entry::Occupied(mut entry) => {
                let (slot_list, rc) = entry.get();
                let r = updatefn(Some((&slot_list, *rc)));
                match r {
                    Some(value) => {
                        *entry.get_mut() = value;
                    }
                    None => {
                        entry.remove();
                    }
                }
            }
            Entry::Vacant(vacant) => {
                let r = updatefn(None);
                match r {
                    Some(value) => {
                        vacant.insert(value);
                    }
                    None => {}
                }
            }
        }
    }
    /*
    pub fn addref(&self, key: &Pubkey) {
        let mut map = self.disk.write().unwrap();
        if let Entry::Occupied(mut entry) = map.entry(*key) {
            entry.get_mut().1 += 1;
        }
    }

    pub fn unref(&self, key: &Pubkey) {
        let mut map = self.disk.write().unwrap();
        if let Entry::Occupied(mut entry) = map.entry(*key) {
            entry.get_mut().1 -= 1;
        }
    }
    */
}
