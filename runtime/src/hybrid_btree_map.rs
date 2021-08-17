use crate::accounts_index::AccountMapEntry;
use crate::accounts_index::{IsCached, RefCount, SlotList, ACCOUNTS_INDEX_CONFIG_FOR_TESTING};
use crate::bucket_map_holder::{BucketMapWriteHolder};

use crate::pubkey_bins::PubkeyBinCalculator16;
use solana_bucket_map::bucket_map::BucketMap;
use solana_sdk::clock::Slot;
use solana_sdk::pubkey::Pubkey;
use std::collections::btree_map::BTreeMap;
use std::fmt::Debug;
use std::ops::Bound;
use std::ops::{Range, RangeBounds};
use std::sync::Arc;

type K = Pubkey;

#[derive(Clone, Debug)]
pub struct HybridAccountEntry<V: Clone + Debug> {
    entry: V,
    //exists_on_disk: bool,
}
//type V2<T: Clone + Debug> = HybridAccountEntry<T>;
pub type V2<T> = AccountMapEntry<T>;
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
pub type SlotT<T> = (Slot, T);

#[derive(Debug)]
pub struct HybridBTreeMap<V: 'static + Clone + IsCached + Debug> {
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

pub enum HybridEntry<'a, V: 'static + Clone + IsCached + Debug> {
    /// A vacant entry.
    Vacant(HybridVacantEntry<'a, V>),

    /// An occupied entry.
    Occupied(HybridOccupiedEntry<'a, V>),
}

pub struct Keys {
    keys: Vec<K>,
    index: usize,
}

impl Keys {
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

pub struct Values<V: Clone + std::fmt::Debug> {
    values: Vec<SlotList<V>>,
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

pub struct HybridOccupiedEntry<'a, V: 'static + Clone + IsCached + Debug> {
    pubkey: Pubkey,
    entry: V2<V>,
    map: &'a HybridBTreeMap<V>,
}
pub struct HybridVacantEntry<'a, V: 'static + Clone + IsCached + Debug> {
    pubkey: Pubkey,
    map: &'a HybridBTreeMap<V>,
}

impl<'a, V: 'a + Clone + IsCached + Debug> HybridOccupiedEntry<'a, V> {
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

        self.map
            .disk
            .addref(&self.pubkey, self.entry.ref_count, &self.entry.slot_list);
        //error!("addref: {}, {}, {:?}", self.pubkey, self.entry.ref_count(), result);
    }
    pub fn unref(&mut self) {
        self.entry.ref_count -= 1;
        self.map
            .disk
            .unref(&self.pubkey, self.entry.ref_count, &self.entry.slot_list);
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

impl<'a, V: 'a + Clone + Debug + IsCached> HybridVacantEntry<'a, V> {
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

impl<V: IsCached> HybridBTreeMap<V> {
    /// Creates an empty `BTreeMap`.
    pub fn new2(bucket_map: &Arc<BucketMapWriteHolder<V>>, bin_index: usize, bins: usize) -> Self {
        Self {
            in_memory: BTreeMap::default(),
            disk: bucket_map.clone(),
            bin_index,
            bins: bins, //bucket_map.num_buckets(),
        }
    }

    pub fn new_for_testing() -> Self {
        let map = Self::new_bucket_map(ACCOUNTS_INDEX_CONFIG_FOR_TESTING);
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
    fn bound<'a, T>(bound: Bound<&'a T>, unbounded: &'a T) -> &'a T {
        match bound {
            Bound::Included(b) | Bound::Excluded(b) => b,
            _ => unbounded,
        }
    }
    pub fn range<R>(&self, range: Option<R>) -> Vec<(Pubkey, SlotList<V>)>
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
        keys.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        keys
    }

    pub fn keys2(&self) -> Keys {
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
    pub fn values(&self) -> Values<V> {
        let num_buckets = self.disk.num_buckets();
        if self.bin_index != 0 && self.disk.unified_backing {
            return Values {
                values: vec![],
                index: 0,
            };
        }
        // todo: this may be unsafe if we are asking for things with an update cache active. thankfully, we only call values at startup right now
        let start = num_buckets * self.bin_index / self.bins;
        let end = num_buckets * (self.bin_index + 1) / self.bins;
        let len = (start..end)
            .into_iter()
            .map(|ix| self.disk.bucket_len(ix) as usize)
            .sum::<usize>();
        let mut values = Vec::with_capacity(len);
        (start..end).into_iter().for_each(|ix| {
            values.append(
                &mut self
                    .disk
                    .values(ix, None::<&Range<Pubkey>>)
                    .unwrap_or_default(),
            )
        });
        //error!("getting values: {}, bin: {}, bins: {}, start: {}, end: {}", values.len(), self.bin_index, self.bins, start, end);
        //keys.sort_unstable();
        if self.bin_index == 0 {
            //error!("getting values: {}, {}, {}", values.len(), start, end);
        }
        Values { values, index: 0 }
    }

    pub fn upsert(
        &self,
        pubkey: &Pubkey,
        new_value: AccountMapEntry<V>,
        reclaims: &mut SlotList<V>,
        reclaims_must_be_empty: bool,
    ) {
        self.disk
            .upsert(pubkey, new_value, reclaims, reclaims_must_be_empty);
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

    pub fn insert2(&mut self, key: K, value: V2<V>) {
        match self.entry(key) {
            HybridEntry::Occupied(_occupied) => {
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
    pub fn len(&self) -> usize {
        self.disk.keys3(self.bin_index, None::<&Range<Pubkey>>).map(|x| x.len()).unwrap_or_default()
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
