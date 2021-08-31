use crate::accounts_index::AccountMapEntry;
use crate::accounts_index::{IsCached, SlotList, ACCOUNTS_INDEX_CONFIG_FOR_TESTING};
use crate::bucket_map_holder::{BucketMapWriteHolder, K};

use crate::pubkey_bins::PubkeyBinCalculator16;
use solana_bucket_map::bucket_map::BucketMap;
use solana_sdk::clock::Slot;
use solana_sdk::pubkey::Pubkey;
use std::fmt::Debug;
// use std::ops::Bound;
use std::ops::{Range, RangeBounds};
use std::sync::Arc;

pub type V2<T> = AccountMapEntry<T>;
pub type SlotT<T> = (Slot, T);

#[derive(Debug)]
pub struct HybridBTreeMap<V: IsCached> {
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

impl<V: IsCached> HybridBTreeMap<V> {
    /// Creates an empty `BTreeMap`.
    pub fn new2(bucket_map: &Arc<BucketMapWriteHolder<V>>, bin_index: usize, bins: usize) -> Self {
        Self {
            disk: bucket_map.clone(),
            bin_index,
            bins, //bucket_map.num_buckets(),
        }
    }

    pub fn new_for_testing() -> Self {
        let bins = ACCOUNTS_INDEX_CONFIG_FOR_TESTING.bins.unwrap();
        let map = Self::new_bucket_map(bins);
        Self::new2(&map, 0, 1)
    }

    pub fn new_bucket_map(bins: usize) -> Arc<BucketMapWriteHolder<V>> {
        let buckets = PubkeyBinCalculator16::log_2(bins as u32) as u8; // make more buckets to try to spread things out
                                                                       // 15 hopefully avoids too many files open problem
                                                                       //buckets = std::cmp::min(buckets + 11, 15); // max # that works with open file handles and such
                                                                       //buckets =
                                                                       //error!("creating: {} for {}", buckets, BUCKET_BINS);
        Arc::new(BucketMapWriteHolder::new(BucketMap::new_buckets(buckets)))
    }

    pub fn flush(&self) -> usize {
        let num_buckets = self.disk.num_buckets();
        let mystart = num_buckets * self.bin_index / self.bins;
        let myend = num_buckets * (self.bin_index + 1) / self.bins;
        assert_eq!(myend - mystart, 1, "{}", self.bin_index);
        (mystart..myend)
            .map(|ix| self.disk.flush(ix, false, None).1)
            .sum()

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
    pub fn distribution(&self) {
        self.disk.distribution();
    }
    /*
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

    pub fn iter<R>(&self, range: R) -> Vec<(K, V2<V>)>
    where
        R: RangeBounds<Pubkey>,
    {
        self.disk.range(self.bin_index, Some(&range))
    }

    pub fn keys(&self) -> Keys {
        // used still?
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
                    .keys3(ix, None::<&Range<Pubkey>>)
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
        self.disk.get(key)
    }
    pub fn remove(&mut self, key: &K) {
        self.disk.delete_key(key); //.map(|x| x.entry)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn len(&self) -> usize {
        self.disk
            .keys3(self.bin_index, None::<&Range<Pubkey>>)
            .map(|x| x.len())
            .unwrap_or_default()
    }

    pub fn set_startup(&self, startup: bool) {
        self.disk.set_startup(startup);
    }

    pub fn update_or_insert_async(&self, pubkey: Pubkey, new_entry: AccountMapEntry<V>) {
        self.disk
            .update_or_insert_async(self.bin_index, pubkey, new_entry);
    }
    pub fn dump_metrics(&self) {
        self.disk.dump_metrics();
    }
}
