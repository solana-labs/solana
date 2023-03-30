use {
    crate::{
        bucket_item::BucketItem,
        bucket_map::BucketMapError,
        bucket_stats::BucketMapStats,
        bucket_storage::{BucketOccupied, BucketStorage, DEFAULT_CAPACITY_POW2},
        index_entry::{DataBucket, IndexBucket, IndexEntry, IndexEntryPlaceInBucket},
        MaxSearch, RefCount,
    },
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
        ops::RangeBounds,
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex,
        },
    },
};

pub struct ReallocatedItems<I: BucketOccupied, D: BucketOccupied> {
    // Some if the index was reallocated
    // u64 is random associated with the new index
    pub index: Option<(u64, BucketStorage<I>)>,
    // Some for a data bucket reallocation
    // u64 is data bucket index
    pub data: Option<(u64, BucketStorage<D>)>,
}

impl<I: BucketOccupied, D: BucketOccupied> Default for ReallocatedItems<I, D> {
    fn default() -> Self {
        Self {
            index: None,
            data: None,
        }
    }
}

pub struct Reallocated<I: BucketOccupied, D: BucketOccupied> {
    /// > 0 if reallocations are encoded
    pub active_reallocations: AtomicUsize,

    /// actual reallocated bucket
    /// mutex because bucket grow code runs with a read lock
    pub items: Mutex<ReallocatedItems<I, D>>,
}

impl<I: BucketOccupied, D: BucketOccupied> Default for Reallocated<I, D> {
    fn default() -> Self {
        Self {
            active_reallocations: AtomicUsize::default(),
            items: Mutex::default(),
        }
    }
}

impl<I: BucketOccupied, D: BucketOccupied> Reallocated<I, D> {
    /// specify that a reallocation has occurred
    pub fn add_reallocation(&self) {
        assert_eq!(
            0,
            self.active_reallocations.fetch_add(1, Ordering::Relaxed),
            "Only 1 reallocation can occur at a time"
        );
    }
    /// Return true IFF a reallocation has occurred.
    /// Calling this takes conceptual ownership of the reallocation encoded in the struct.
    pub fn get_reallocated(&self) -> bool {
        self.active_reallocations
            .compare_exchange(1, 0, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
}

// >= 2 instances of BucketStorage per 'bucket' in the bucket map. 1 for index, >= 1 for data
pub struct Bucket<T: 'static> {
    drives: Arc<Vec<PathBuf>>,
    //index
    pub index: BucketStorage<IndexBucket<T>>,
    //random offset for the index
    random: u64,
    //storage buckets to store SlotSlice up to a power of 2 in len
    pub data: Vec<BucketStorage<DataBucket>>,
    stats: Arc<BucketMapStats>,

    pub reallocated: Reallocated<IndexBucket<T>, DataBucket>,
}

impl<'b, T: Clone + Copy + 'static> Bucket<T> {
    pub fn new(
        drives: Arc<Vec<PathBuf>>,
        max_search: MaxSearch,
        stats: Arc<BucketMapStats>,
        count: Arc<AtomicU64>,
    ) -> Self {
        let index = BucketStorage::new(
            Arc::clone(&drives),
            1,
            std::mem::size_of::<IndexEntry<T>>() as u64,
            max_search,
            Arc::clone(&stats.index),
            count,
        );
        stats.index.resize_grow(0, index.capacity_bytes());

        Self {
            random: thread_rng().gen(),
            drives,
            index,
            data: vec![],
            stats,
            reallocated: Reallocated::default(),
        }
    }

    pub fn keys(&self) -> Vec<Pubkey> {
        let mut rv = vec![];
        for i in 0..self.index.capacity() {
            if self.index.is_free(i) {
                continue;
            }
            let ix: &IndexEntry<T> = self.index.get(i);
            rv.push(ix.key);
        }
        rv
    }

    pub fn items_in_range<R>(&self, range: &Option<&R>) -> Vec<BucketItem<T>>
    where
        R: RangeBounds<Pubkey>,
    {
        let mut result = Vec::with_capacity(self.index.count.load(Ordering::Relaxed) as usize);
        for i in 0..self.index.capacity() {
            let ii = i % self.index.capacity();
            if self.index.is_free(ii) {
                continue;
            }
            let ix = IndexEntryPlaceInBucket::new(ii);
            let key = ix.key(&self.index);
            if range.map(|r| r.contains(key)).unwrap_or(true) {
                let val = ix.read_value(&self.index, &self.data);
                result.push(BucketItem {
                    pubkey: *key,
                    ref_count: ix.ref_count(&self.index),
                    slot_list: val.map(|(v, _ref_count)| v.to_vec()).unwrap_or_default(),
                });
            }
        }
        result
    }

    pub fn find_index_entry(&self, key: &Pubkey) -> Option<(IndexEntryPlaceInBucket<T>, u64)> {
        Self::bucket_find_index_entry(&self.index, key, self.random)
    }

    /// find an entry for `key`
    /// if entry exists, return the entry along with the index of the existing entry
    /// if entry does not exist, return just the index of an empty entry appropriate for this key
    /// returns (existing entry, index of the found or empty entry)
    fn find_index_entry_mut(
        index: &mut BucketStorage<IndexBucket<T>>,
        key: &Pubkey,
        random: u64,
    ) -> Result<(Option<IndexEntryPlaceInBucket<T>>, u64), BucketMapError> {
        let ix = Self::bucket_index_ix(index, key, random);
        let mut first_free = None;
        let mut m = Measure::start("bucket_find_index_entry_mut");
        let capacity = index.capacity();
        for i in ix..ix + index.max_search() {
            let ii = i % capacity;
            if index.is_free(ii) {
                if first_free.is_none() {
                    first_free = Some(ii);
                }
                continue;
            }
            let elem = IndexEntryPlaceInBucket::new(ii);
            if elem.key(index) == key {
                m.stop();

                index
                    .stats
                    .find_index_entry_mut_us
                    .fetch_add(m.as_us(), Ordering::Relaxed);
                return Ok((Some(elem), ii));
            }
        }
        m.stop();
        index
            .stats
            .find_index_entry_mut_us
            .fetch_add(m.as_us(), Ordering::Relaxed);
        match first_free {
            Some(ii) => Ok((None, ii)),
            None => Err(BucketMapError::IndexNoSpace(index.capacity_pow2)),
        }
    }

    fn bucket_find_index_entry(
        index: &BucketStorage<IndexBucket<T>>,
        key: &Pubkey,
        random: u64,
    ) -> Option<(IndexEntryPlaceInBucket<T>, u64)> {
        let ix = Self::bucket_index_ix(index, key, random);
        for i in ix..ix + index.max_search() {
            let ii = i % index.capacity();
            if index.is_free(ii) {
                continue;
            }
            let elem = IndexEntryPlaceInBucket::new(ii);
            if elem.key(index) == key {
                return Some((elem, ii));
            }
        }
        None
    }

    fn bucket_create_key(
        index: &mut BucketStorage<IndexBucket<T>>,
        key: &Pubkey,
        random: u64,
        is_resizing: bool,
    ) -> Result<u64, BucketMapError> {
        let mut m = Measure::start("bucket_create_key");
        let ix = Self::bucket_index_ix(index, key, random);
        for i in ix..ix + index.max_search() {
            let ii = i % index.capacity();
            if !index.is_free(ii) {
                continue;
            }
            index.occupy(ii, is_resizing).unwrap();
            // These fields will be overwritten after allocation by callers.
            // Since this part of the mmapped file could have previously been used by someone else, there can be garbage here.
            IndexEntryPlaceInBucket::new(ii).init(index, key);
            //debug!(                "INDEX ALLOC {:?} {} {} {}",                key, ii, index.capacity, elem_uid            );
            m.stop();
            index
                .stats
                .find_index_entry_mut_us
                .fetch_add(m.as_us(), Ordering::Relaxed);
            return Ok(ii);
        }
        m.stop();
        index
            .stats
            .find_index_entry_mut_us
            .fetch_add(m.as_us(), Ordering::Relaxed);
        Err(BucketMapError::IndexNoSpace(index.capacity_pow2))
    }

    pub fn read_value(&self, key: &Pubkey) -> Option<(&[T], RefCount)> {
        //debug!("READ_VALUE: {:?}", key);
        let (elem, _) = self.find_index_entry(key)?;
        elem.read_value(&self.index, &self.data)
    }

    pub fn try_write(
        &mut self,
        key: &Pubkey,
        data: impl Iterator<Item = &'b T>,
        data_len: usize,
        ref_count: RefCount,
    ) -> Result<(), BucketMapError> {
        let best_fit_bucket = IndexEntry::<T>::data_bucket_from_num_slots(data_len as u64);
        if self.data.get(best_fit_bucket as usize).is_none() {
            // fail early if the data bucket we need doesn't exist - we don't want the index entry partially allocated
            return Err(BucketMapError::DataNoSpace((best_fit_bucket, 0)));
        }
        let max_search = self.index.max_search();
        let (elem, elem_ix) = Self::find_index_entry_mut(&mut self.index, key, self.random)?;
        let elem = if let Some(elem) = elem {
            elem
        } else {
            let is_resizing = false;
            self.index.occupy(elem_ix, is_resizing).unwrap();
            let elem_allocate = IndexEntryPlaceInBucket::new(elem_ix);
            // These fields will be overwritten after allocation by callers.
            // Since this part of the mmapped file could have previously been used by someone else, there can be garbage here.
            elem_allocate.init(&mut self.index, key);
            elem_allocate
        };

        elem.set_ref_count(&mut self.index, ref_count);
        let bucket_ix = elem.data_bucket_ix(&self.index);
        let num_slots = data_len as u64;
        if best_fit_bucket == bucket_ix && elem.num_slots(&self.index) > 0 {
            let current_bucket = &mut self.data[bucket_ix as usize];
            // in place update
            let elem_loc = elem.data_loc(&self.index, current_bucket);
            assert!(!current_bucket.is_free(elem_loc));
            let slice: &mut [T] = current_bucket.get_mut_cell_slice(elem_loc, data_len as u64);
            elem.set_num_slots(&mut self.index, num_slots);

            slice.iter_mut().zip(data).for_each(|(dest, src)| {
                *dest = *src;
            });
            Ok(())
        } else {
            // need to move the allocation to a best fit spot
            let best_bucket = &self.data[best_fit_bucket as usize];
            let current_bucket = &self.data[bucket_ix as usize];
            let cap_power = best_bucket.capacity_pow2;
            let cap = best_bucket.capacity();
            let pos = thread_rng().gen_range(0, cap);
            // max search is increased here by a lot for this search. The idea is that we just have to find an empty bucket somewhere.
            // We don't mind waiting on a new write (by searching longer). Writing is done in the background only.
            // Wasting space by doubling the bucket size is worse behavior. We expect more
            // updates and fewer inserts, so we optimize for more compact data.
            // We can accomplish this by increasing how many locations we're willing to search for an empty data cell.
            // For the index bucket, it is more like a hash table and we have to exhaustively search 'max_search' to prove an item does not exist.
            // And we do have to support the 'does not exist' case with good performance. So, it makes sense to grow the index bucket when it is too large.
            // For data buckets, the offset is stored in the index, so it is directly looked up. So, the only search is on INSERT or update to a new sized value.
            for i in pos..pos + (max_search * 10).min(cap) {
                let ix = i % cap;
                if best_bucket.is_free(ix) {
                    let elem_loc = elem.data_loc(&self.index, current_bucket);
                    let old_slots = elem.num_slots(&self.index);
                    let multiple_slots = elem.get_multiple_slots_mut(&mut self.index);
                    multiple_slots.set_storage_offset(ix);
                    multiple_slots
                        .set_storage_capacity_when_created_pow2(best_bucket.capacity_pow2);
                    elem.set_num_slots(&mut self.index, num_slots);
                    if old_slots > 0 {
                        let current_bucket = &mut self.data[bucket_ix as usize];
                        current_bucket.free(elem_loc);
                    }
                    //debug!(                        "DATA ALLOC {:?} {} {} {}",                        key, elem.data_location, best_bucket.capacity, elem_uid                    );
                    if num_slots > 0 {
                        let best_bucket = &mut self.data[best_fit_bucket as usize];
                        best_bucket.occupy(ix, false).unwrap();
                        let slice = best_bucket.get_mut_cell_slice(ix, num_slots);
                        slice.iter_mut().zip(data).for_each(|(dest, src)| {
                            *dest = *src;
                        });
                    }
                    return Ok(());
                }
            }
            Err(BucketMapError::DataNoSpace((best_fit_bucket, cap_power)))
        }
    }

    pub fn delete_key(&mut self, key: &Pubkey) {
        if let Some((elem, elem_ix)) = self.find_index_entry(key) {
            if elem.num_slots(&self.index) > 0 {
                let ix = elem.data_bucket_ix(&self.index) as usize;
                let data_bucket = &self.data[ix];
                let loc = elem.data_loc(&self.index, data_bucket);
                let data_bucket = &mut self.data[ix];
                //debug!(                    "DATA FREE {:?} {} {} {}",                    key, elem.data_location, data_bucket.capacity, elem_uid                );
                data_bucket.free(loc);
            }
            //debug!("INDEX FREE {:?} {}", key, elem_uid);
            self.index.free(elem_ix);
        }
    }

    pub fn grow_index(&self, current_capacity_pow2: u8) {
        if self.index.capacity_pow2 == current_capacity_pow2 {
            let mut m = Measure::start("grow_index");
            //debug!("GROW_INDEX: {}", current_capacity_pow2);
            let increment = 1;
            for i in increment.. {
                //increasing the capacity by ^4 reduces the
                //likelihood of a re-index collision of 2^(max_search)^2
                //1 in 2^32
                let mut index = BucketStorage::new_with_capacity(
                    Arc::clone(&self.drives),
                    1,
                    std::mem::size_of::<IndexEntry<T>>() as u64,
                    // *2 causes rapid growth of index buckets
                    self.index.capacity_pow2 + i, // * 2,
                    self.index.max_search,
                    Arc::clone(&self.stats.index),
                    Arc::clone(&self.index.count),
                );
                let random = thread_rng().gen();
                let mut valid = true;
                for ix in 0..self.index.capacity() {
                    if !self.index.is_free(ix) {
                        let elem: &IndexEntry<T> = self.index.get(ix);
                        let new_ix = Self::bucket_create_key(&mut index, &elem.key, random, true);
                        if new_ix.is_err() {
                            valid = false;
                            break;
                        }
                        let new_ix = new_ix.unwrap();
                        let new_elem: &mut IndexEntry<T> = index.get_mut(new_ix);
                        *new_elem = *elem;
                        /*
                        let dbg_elem: IndexEntry = *new_elem;
                        assert_eq!(
                            Self::bucket_find_index_entry(&index, &elem.key, random).unwrap(),
                            (&dbg_elem, new_ix)
                        );
                        */
                    }
                }
                if valid {
                    self.stats.index.update_max_size(index.capacity());
                    let mut items = self.reallocated.items.lock().unwrap();
                    items.index = Some((random, index));
                    self.reallocated.add_reallocation();
                    break;
                }
            }
            m.stop();
            self.stats.index.resizes.fetch_add(1, Ordering::Relaxed);
            self.stats
                .index
                .resize_us
                .fetch_add(m.as_us(), Ordering::Relaxed);
        }
    }

    pub fn apply_grow_index(&mut self, random: u64, index: BucketStorage<IndexBucket<T>>) {
        self.stats
            .index
            .resize_grow(self.index.capacity_bytes(), index.capacity_bytes());

        self.random = random;
        self.index = index;
    }

    fn elem_size() -> u64 {
        std::mem::size_of::<T>() as u64
    }

    fn add_data_bucket(&mut self, bucket: BucketStorage<DataBucket>) {
        self.stats.data.file_count.fetch_add(1, Ordering::Relaxed);
        self.stats.data.resize_grow(0, bucket.capacity_bytes());
        self.data.push(bucket);
    }

    pub fn apply_grow_data(&mut self, ix: usize, bucket: BucketStorage<DataBucket>) {
        if self.data.get(ix).is_none() {
            for i in self.data.len()..ix {
                // insert empty data buckets
                self.add_data_bucket(BucketStorage::new(
                    Arc::clone(&self.drives),
                    1 << i,
                    Self::elem_size(),
                    self.index.max_search,
                    Arc::clone(&self.stats.data),
                    Arc::default(),
                ));
            }
            self.add_data_bucket(bucket);
        } else {
            let data_bucket = &mut self.data[ix];
            self.stats
                .data
                .resize_grow(data_bucket.capacity_bytes(), bucket.capacity_bytes());
            self.data[ix] = bucket;
        }
    }

    /// grow a data bucket
    /// The application of the new bucket is deferred until the next write lock.
    pub fn grow_data(&self, data_index: u64, current_capacity_pow2: u8) {
        let new_bucket = BucketStorage::new_resized(
            &self.drives,
            self.index.max_search,
            self.data.get(data_index as usize),
            std::cmp::max(current_capacity_pow2 + 1, DEFAULT_CAPACITY_POW2),
            1 << data_index,
            Self::elem_size(),
            &self.stats.data,
        );
        self.reallocated.add_reallocation();
        let mut items = self.reallocated.items.lock().unwrap();
        items.data = Some((data_index, new_bucket));
    }

    fn bucket_index_ix(index: &BucketStorage<IndexBucket<T>>, key: &Pubkey, random: u64) -> u64 {
        let mut s = DefaultHasher::new();
        key.hash(&mut s);
        //the locally generated random will make it hard for an attacker
        //to deterministically cause all the pubkeys to land in the same
        //location in any bucket on all validators
        random.hash(&mut s);
        let ix = s.finish();
        ix % index.capacity()
        //debug!(            "INDEX_IX: {:?} uid:{} loc: {} cap:{}",            key,            uid,            location,            index.capacity()        );
    }

    /// grow the appropriate piece. Note this takes an immutable ref.
    /// The actual grow is set into self.reallocated and applied later on a write lock
    pub fn grow(&self, err: BucketMapError) {
        match err {
            BucketMapError::DataNoSpace((data_index, current_capacity_pow2)) => {
                //debug!("GROWING SPACE {:?}", (data_index, current_capacity_pow2));
                self.grow_data(data_index, current_capacity_pow2);
            }
            BucketMapError::IndexNoSpace(current_capacity_pow2) => {
                //debug!("GROWING INDEX {}", sz);
                self.grow_index(current_capacity_pow2);
            }
        }
    }

    /// if a bucket was resized previously with a read lock, then apply that resize now
    pub fn handle_delayed_grows(&mut self) {
        if self.reallocated.get_reallocated() {
            // swap out the bucket that was resized previously with a read lock
            let mut items = std::mem::take(&mut *self.reallocated.items.lock().unwrap());

            if let Some((random, bucket)) = items.index.take() {
                self.apply_grow_index(random, bucket);
            } else {
                // data bucket
                let (i, new_bucket) = items.data.take().unwrap();
                self.apply_grow_data(i as usize, new_bucket);
            }
        }
    }

    pub fn insert(&mut self, key: &Pubkey, value: (&[T], RefCount)) {
        let (new, refct) = value;
        loop {
            let rv = self.try_write(key, new.iter(), new.len(), refct);
            match rv {
                Ok(_) => return,
                Err(err) => {
                    self.grow(err);
                    self.handle_delayed_grows();
                }
            }
        }
    }

    pub fn update<F>(&mut self, key: &Pubkey, mut updatefn: F)
    where
        F: FnMut(Option<(&[T], RefCount)>) -> Option<(Vec<T>, RefCount)>,
    {
        let current = self.read_value(key);
        let new = updatefn(current);
        if new.is_none() {
            self.delete_key(key);
            return;
        }
        let (new, refct) = new.unwrap();
        self.insert(key, (&new, refct));
    }
}
