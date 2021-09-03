use crate::accounts_index::AccountMapEntry;
use crate::accounts_index::{IsCached, SlotList, ACCOUNTS_INDEX_CONFIG_FOR_TESTING};
use crate::bucket_map_holder::{BucketMapHolder, K};

use crate::pubkey_bins::PubkeyBinCalculator16;
use solana_bucket_map::bucket_map::BucketMap;
use solana_sdk::clock::Slot;
use solana_sdk::pubkey::Pubkey;
use std::fmt::Debug;
use std::ops::{Range, RangeBounds};
use std::sync::Arc;

pub type V2<T> = AccountMapEntry<T>;
pub type SlotT<T> = (Slot, T);

#[derive(Debug)]
pub struct InMemAccountsIndex<V: IsCached> {
    disk: Arc<BucketMapHolder<V>>,
    bin_index: usize,
    bins: usize,
}

pub struct Keys {
    keys: Vec<K>,
    index: usize,
}

impl Keys {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn len(&self) -> usize {
        self.keys.len()
    }
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

impl<V: IsCached> InMemAccountsIndex<V> {
    pub fn new(bucket_map: &Arc<BucketMapHolder<V>>, bin_index: usize, bins: usize) -> Self {
        Self {
            disk: bucket_map.clone(),
            bin_index,
            bins, //bucket_map.num_buckets(),
        }
    }

    pub fn new_for_testing() -> Self {
        let bins = ACCOUNTS_INDEX_CONFIG_FOR_TESTING.bins.unwrap();
        let map = Self::new_bucket_map(bins);
        Self::new(&map, 0, 1)
    }

    pub fn new_bucket_map(bins: usize) -> Arc<BucketMapHolder<V>> {
        let buckets = PubkeyBinCalculator16::log_2(bins as u32) as u8;
        Arc::new(BucketMapHolder::new(BucketMap::new_buckets(buckets)))
    }

    /*
    todo:
    fn bound<'a, T>(bound: Bound<&'a T>, unbounded: &'a T) -> &'a T {
        match bound {
            Bound::Included(b) | Bound::Excluded(b) => b,
            _ => unbounded,
        }
    }
    pub fn range2<R>(&self, range: Option<R>, all_and_unsorted: bool) -> Vec<(Pubkey, SlotList<V>)>
    where
        R: RangeBounds<Pubkey>,
    {
        //self.disk.range.fetch_add(1, Ordering::Relaxed);

        let num_buckets = self.disk.num_buckets();
        if self.bin_index != 0 && self.disk.unified_backing {
            return vec![];
        }
        let mut start = 0;
        let mut end = num_buckets;
        if let Some(range) = &range {
            let default = Pubkey::default();
            let max = Pubkey::new(&[0xff; 32]);
            let start_bound = Self::bound(range.start_bound(), &default);
            start = self.disk.bucket_ix(start_bound);
            // end is exclusive, so it is end + 1 we care about here
            let end_bound = Self::bound(range.end_bound(), &max);
            end = std::cmp::min(num_buckets, 1 + self.disk.bucket_ix(end_bound)); // ugly
            assert!(
                start_bound <= end_bound,
                "range start is greater than range end"
            );
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
            let mut ks = self.disk.range(ix, range.as_ref());
            keys.append(&mut ks);
        });
        if !all_and_unsorted {
            keys.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        }
        panic!("");

        //keys
    }
    */

    pub fn hold_range_in_memory<R>(&self, range: &R, hold: bool)
    where
        R: RangeBounds<Pubkey>,
    {
        self.disk.hold_range_in_memory(self.bin_index, range, hold);
    }

    pub fn iter<R>(&self, range: Option<&R>) -> Vec<(K, V2<V>)>
    where
        R: RangeBounds<Pubkey>,
    {
        self.disk.range(self.bin_index, range)
    }

    pub fn keys(&self) -> Keys {
        let num_buckets = self.disk.num_buckets();
        let start = num_buckets * self.bin_index / self.bins;
        let end = num_buckets * (self.bin_index + 1) / self.bins;
        let len = (start..end)
            .into_iter()
            .map(|ix| self.disk.bucket_len(ix) as usize)
            .sum::<usize>();
        let mut keys = Vec::with_capacity(len);
        let _len = (start..end).into_iter().for_each(|ix| {
            keys.append(
                &mut self
                    .disk
                    .keys(ix, None::<&Range<Pubkey>>)
                    .unwrap_or_default(),
            )
        });
        keys.sort_unstable();
        Keys { keys, index: 0 }
    }

    pub fn remove_if_slot_list_empty(&self, key: &Pubkey) -> bool {
        self.disk.remove_if_slot_list_empty(self.bin_index, key)
    }

    pub fn upsert(
        &self,
        pubkey: &Pubkey,
        new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        previous_slot_entry_was_cached: bool,
    ) {
        self.disk
            .upsert(pubkey, new_value, reclaims, previous_slot_entry_was_cached);
    }

    pub fn get(&self, key: &K) -> Option<V2<V>> {
        self.disk.get(self.bin_index, key)
    }
    pub fn remove(&mut self, key: &K) {
        self.disk.delete_key(self.bin_index, key);
    }
    // This is expensive. We could use a counter per bucket to keep track of adds/deletes.
    // But, this is only used for metrics.
    pub fn len_expensive(&self) -> usize {
        self.disk.len_expensive(self.bin_index)
    }

    pub fn set_startup(&self, startup: bool) {
        self.disk.set_startup(startup);
    }

    pub fn update_or_insert_async(&self, pubkey: Pubkey, new_entry: AccountMapEntry<V>) {
        self.disk
            .update_or_insert_async(self.bin_index, pubkey, new_entry);
    }
}
