use crate::accounts_index::{AccountMapEntry, IsCached, SlotList, WriteAccountMapEntry};
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::pubkey_bins::PubkeyBinCalculator16;
use solana_measure::measure::Measure;
use solana_sdk::pubkey::Pubkey;
use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::ops::RangeBounds;
use std::sync::{atomic::Ordering, Arc, RwLock};
pub type K = Pubkey;

use std::collections::{hash_map::Entry as HashMapEntry, HashMap};

pub type WriteCache<V> = HashMap<Pubkey, V>;

type WriteCacheEntryArc<V> = AccountMapEntry<V>;

pub type Cache<V> = Vec<CacheBin<V>>;
pub type CacheBin<V> = RwLock<WriteCache<WriteCacheEntryArc<V>>>;
pub type CacheSlice<'a, V> = &'a [CacheBin<V>];

// one instance represents the entire accounts index
// likely held in an arc
#[derive(Debug)]
pub struct BucketMapHolder<V: IsCached> {
    // in-mem cache for actively used items in the cache, binned
    // for the in-mem only version, this is the ONLY backing store
    cache: Cache<V>,
    bins: usize,
    binner: PubkeyBinCalculator16,
    stats: BucketMapHolderStats,
}

impl<V: IsCached> BucketMapHolder<V> {
    fn maybe_report_stats(&self) -> bool {
        self.stats.report_stats(&self.cache)
    }

    pub fn new(bins: usize) -> Self {
        let cache = (0..bins)
            .map(|_i| RwLock::new(WriteCache::default()))
            .collect();
        let binner = PubkeyBinCalculator16::new(bins);

        Self {
            stats: BucketMapHolderStats::default(),
            cache,
            bins,
            binner,
        }
    }
    pub fn len_bucket(&self, ix: usize) -> usize {
        self.cache[ix].read().unwrap().len()
    }

    pub fn bucket_len(&self, ix: usize) -> u64 {
        self.cache[ix].read().unwrap().len() as u64
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
                if index_entry.get().slot_list.read().unwrap().is_empty() {
                    index_entry.remove();
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
        ix: usize,
        key: &Pubkey,
        new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        previous_slot_entry_was_cached: bool,
    ) {
        let m1 = Measure::start("upsert");
        let wc = &mut self.cache[ix].write().unwrap();
        let res = wc.entry(*key);
        match res {
            HashMapEntry::Occupied(occupied) => {
                let item = occupied.get();
                self.upsert_item_in_cache(
                    item,
                    new_value,
                    reclaims,
                    previous_slot_entry_was_cached,
                    m1,
                );
            }
            HashMapEntry::Vacant(vacant) => {
                // not on disk, not in cache, so add to cache
                self.stats.inserts.fetch_add(1, Ordering::Relaxed);
                vacant.insert(new_value);
            }
        }
    }

    // return None if item was created new
    // if entry for pubkey already existed, return Some(entry).
    // this will be refactored to be async later so caller can't know if item exists on disk or not yet
    // similar to upsert
    // only called at startup
    pub fn upsert_with_lock_pubkey_result(
        &self,
        ix: usize,
        pubkey: Pubkey,
        new_entry: AccountMapEntry<V>,
    ) -> Option<Pubkey> {
        let m1 = Measure::start("upsert_with_lock_pubkey_result");
        let wc = &mut self.cache[ix].write().unwrap();
        let res = wc.entry(pubkey);
        match res {
            HashMapEntry::Occupied(occupied) => {
                let previous_slot_entry_was_cached = false; // doesn't matter what we pass here
                let mut reclaims = Vec::default();
                self.upsert_item_in_cache(
                    occupied.get(),
                    new_entry,
                    &mut reclaims,
                    previous_slot_entry_was_cached,
                    m1,
                );
                Some(*occupied.key())
            }
            HashMapEntry::Vacant(vacant) => {
                self.stats.inserts.fetch_add(1, Ordering::Relaxed);
                vacant.insert(new_entry);
                None
            }
        }
    }

    // note this returns unsorted
    pub fn range<R>(&self, ix: usize, range: Option<&R>) -> Vec<(Pubkey, AccountMapEntry<V>)>
    where
        R: RangeBounds<Pubkey>,
    {
        self.stats.range.fetch_add(1, Ordering::Relaxed);
        self.maybe_report_stats();
        let lock = self.cache[ix].read().unwrap();
        let mut result = Vec::with_capacity(lock.len());
        for (k, v) in lock.iter() {
            if range.map(|range| range.contains(k)).unwrap_or(true) {
                result.push((*k, v.clone()));
            }
        }
        result
    }

    pub fn get(&self, ix: usize, key: &Pubkey) -> Option<AccountMapEntry<V>> {
        let mut m1 = Measure::start("get");
        let wc = &mut self.cache[ix].read().unwrap();
        let item_in_cache = wc.get(key);
        m1.stop();
        if let Some(item_in_cache) = item_in_cache {
            self.stats
                .get_cache_us
                .fetch_add(m1.as_ns(), Ordering::Relaxed);
            self.stats.gets_from_cache.fetch_add(1, Ordering::Relaxed);
            return Some(item_in_cache.clone());
        } else {
            self.stats
                .gets_from_disk_empty
                .fetch_add(1, Ordering::Relaxed);
        }
        None
    }

    pub fn delete_key(&self, ix: usize, key: &Pubkey) {
        self.stats.deletes.fetch_add(1, Ordering::Relaxed);
        {
            let wc = &mut self.cache[ix].write().unwrap();
            wc.remove(key);
        }
    }
}
