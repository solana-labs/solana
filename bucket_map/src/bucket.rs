use {
    crate::{
        bucket_item::BucketItem,
        bucket_map::BucketMapError,
        bucket_stats::BucketMapStats,
        bucket_storage::{BucketStorage, Uid, DEFAULT_CAPACITY_POW2},
        index_entry::IndexEntry,
        MaxSearch, RefCount,
    },
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
        marker::PhantomData,
        ops::RangeBounds,
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex,
        },
    },
};

#[derive(Default)]
pub struct ReallocatedItems {
    // Some if the index was reallocated
    // u64 is random associated with the new index
    pub index: Option<(u64, BucketStorage)>,
    // Some for a data bucket reallocation
    // u64 is data bucket index
    pub data: Option<(u64, BucketStorage)>,
}

#[derive(Default)]
pub struct Reallocated {
    /// > 0 if reallocations are encoded
    pub active_reallocations: AtomicUsize,

    /// actual reallocated bucket
    /// mutex because bucket grow code runs with a read lock
    pub items: Mutex<ReallocatedItems>,
}

impl Reallocated {
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
pub struct Bucket<T> {
    drives: Arc<Vec<PathBuf>>,
    //index
    pub index: BucketStorage,
    //random offset for the index
    random: u64,
    //storage buckets to store SlotSlice up to a power of 2 in len
    pub data: Vec<BucketStorage>,
    _phantom: PhantomData<T>,
    stats: Arc<BucketMapStats>,

    pub reallocated: Reallocated,
}

impl<T: Clone + Copy> Bucket<T> {
    pub fn new(
        drives: Arc<Vec<PathBuf>>,
        max_search: MaxSearch,
        stats: Arc<BucketMapStats>,
        count: Arc<AtomicU64>,
    ) -> Self {
        let index = BucketStorage::new(
            Arc::clone(&drives),
            1,
            std::mem::size_of::<IndexEntry>() as u64,
            max_search,
            Arc::clone(&stats.index),
            count,
        );
        Self {
            random: thread_rng().gen(),
            drives,
            index,
            data: vec![],
            _phantom: PhantomData::default(),
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
            let ix: &IndexEntry = self.index.get(i);
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
            let ix: &IndexEntry = self.index.get(ii);
            let key = ix.key;
            if range.map(|r| r.contains(&key)).unwrap_or(true) {
                let val = ix.read_value(self);
                result.push(BucketItem {
                    pubkey: key,
                    ref_count: ix.ref_count(),
                    slot_list: val.map(|(v, _ref_count)| v.to_vec()).unwrap_or_default(),
                });
            }
        }
        result
    }

    pub fn find_entry(&self, key: &Pubkey) -> Option<(&IndexEntry, u64)> {
        Self::bucket_find_entry(&self.index, key, self.random)
    }

    fn find_entry_mut<'a>(
        &'a self,
        key: &Pubkey,
    ) -> Result<(bool, &'a mut IndexEntry, u64), BucketMapError> {
        let ix = Self::bucket_index_ix(&self.index, key, self.random);
        let mut first_free = None;
        let mut m = Measure::start("bucket_find_entry_mut");
        for i in ix..ix + self.index.max_search() {
            let ii = i % self.index.capacity();
            if self.index.is_free(ii) {
                if first_free.is_none() {
                    first_free = Some(ii);
                }
                continue;
            }
            let elem: &mut IndexEntry = self.index.get_mut(ii);
            if elem.key == *key {
                m.stop();
                self.stats
                    .index
                    .find_entry_mut_us
                    .fetch_add(m.as_us(), Ordering::Relaxed);
                return Ok((true, elem, ii));
            }
        }
        m.stop();
        self.stats
            .index
            .find_entry_mut_us
            .fetch_add(m.as_us(), Ordering::Relaxed);
        match first_free {
            Some(ii) => {
                let elem: &mut IndexEntry = self.index.get_mut(ii);
                Ok((false, elem, ii))
            }
            None => Err(self.index_no_space()),
        }
    }

    fn bucket_find_entry<'a>(
        index: &'a BucketStorage,
        key: &Pubkey,
        random: u64,
    ) -> Option<(&'a IndexEntry, u64)> {
        let ix = Self::bucket_index_ix(index, key, random);
        for i in ix..ix + index.max_search() {
            let ii = i % index.capacity();
            if index.is_free(ii) {
                continue;
            }
            let elem: &IndexEntry = index.get(ii);
            if elem.key == *key {
                return Some((elem, ii));
            }
        }
        None
    }

    fn bucket_create_key(
        index: &mut BucketStorage,
        key: &Pubkey,
        elem_uid: Uid,
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
            index.allocate(ii, elem_uid, is_resizing).unwrap();
            let elem: &mut IndexEntry = index.get_mut(ii);
            // These fields will be overwritten after allocation by callers.
            // Since this part of the mmapped file could have previously been used by someone else, there can be garbage here.
            elem.init(key);
            //debug!(                "INDEX ALLOC {:?} {} {} {}",                key, ii, index.capacity, elem_uid            );
            m.stop();
            index
                .stats
                .find_entry_mut_us
                .fetch_add(m.as_us(), Ordering::Relaxed);
            return Ok(ii);
        }
        m.stop();
        index
            .stats
            .find_entry_mut_us
            .fetch_add(m.as_us(), Ordering::Relaxed);
        Err(BucketMapError::IndexNoSpace(index.capacity_pow2))
    }

    pub fn addref(&mut self, key: &Pubkey) -> Option<RefCount> {
        if let Ok((found, elem, _)) = self.find_entry_mut(key) {
            if found {
                elem.ref_count += 1;
                return Some(elem.ref_count);
            }
        }
        None
    }

    pub fn unref(&mut self, key: &Pubkey) -> Option<RefCount> {
        if let Ok((found, elem, _)) = self.find_entry_mut(key) {
            if found {
                elem.ref_count -= 1;
                return Some(elem.ref_count);
            }
        }
        None
    }

    pub fn read_value(&self, key: &Pubkey) -> Option<(&[T], RefCount)> {
        //debug!("READ_VALUE: {:?}", key);
        let (elem, _) = self.find_entry(key)?;
        elem.read_value(self)
    }

    fn index_no_space(&self) -> BucketMapError {
        BucketMapError::IndexNoSpace(self.index.capacity_pow2)
    }

    pub fn try_write(
        &mut self,
        key: &Pubkey,
        data: &[T],
        ref_count: RefCount,
    ) -> Result<(), BucketMapError> {
        let best_fit_bucket = IndexEntry::data_bucket_from_num_slots(data.len() as u64);
        if self.data.get(best_fit_bucket as usize).is_none() {
            // fail early if the data bucket we need doesn't exist - we don't want the index entry partially allocated
            return Err(BucketMapError::DataNoSpace((best_fit_bucket, 0)));
        }
        let (found, elem, elem_ix) = self.find_entry_mut(key)?;
        if !found {
            let is_resizing = false;
            let elem_uid = IndexEntry::key_uid(key);
            self.index.allocate(elem_ix, elem_uid, is_resizing).unwrap();
            // These fields will be overwritten after allocation by callers.
            // Since this part of the mmapped file could have previously been used by someone else, there can be garbage here.
            elem.init(key);
        }
        elem.ref_count = ref_count;
        let elem_uid = self.index.uid_unchecked(elem_ix);
        let bucket_ix = elem.data_bucket_ix();
        let current_bucket = &self.data[bucket_ix as usize];
        let num_slots = data.len() as u64;
        if best_fit_bucket == bucket_ix && elem.num_slots > 0 {
            // in place update
            let elem_loc = elem.data_loc(current_bucket);
            let slice: &mut [T] = current_bucket.get_mut_cell_slice(elem_loc, data.len() as u64);
            assert_eq!(current_bucket.uid(elem_loc), Some(elem_uid));
            elem.num_slots = num_slots;
            slice.copy_from_slice(data);
            Ok(())
        } else {
            // need to move the allocation to a best fit spot
            let best_bucket = &self.data[best_fit_bucket as usize];
            let cap_power = best_bucket.capacity_pow2;
            let cap = best_bucket.capacity();
            let pos = thread_rng().gen_range(0, cap);
            for i in pos..pos + self.index.max_search() {
                let ix = i % cap;
                if best_bucket.is_free(ix) {
                    let elem_loc = elem.data_loc(current_bucket);
                    let old_slots = elem.num_slots;
                    elem.set_storage_offset(ix);
                    elem.set_storage_capacity_when_created_pow2(best_bucket.capacity_pow2);
                    elem.num_slots = num_slots;
                    if old_slots > 0 {
                        let current_bucket = &mut self.data[bucket_ix as usize];
                        current_bucket.free(elem_loc, elem_uid);
                    }
                    //debug!(                        "DATA ALLOC {:?} {} {} {}",                        key, elem.data_location, best_bucket.capacity, elem_uid                    );
                    if num_slots > 0 {
                        let best_bucket = &mut self.data[best_fit_bucket as usize];
                        best_bucket.allocate(ix, elem_uid, false).unwrap();
                        let slice = best_bucket.get_mut_cell_slice(ix, num_slots);
                        slice.copy_from_slice(data);
                    }
                    return Ok(());
                }
            }
            Err(BucketMapError::DataNoSpace((best_fit_bucket, cap_power)))
        }
    }

    pub fn delete_key(&mut self, key: &Pubkey) {
        if let Some((elem, elem_ix)) = self.find_entry(key) {
            let elem_uid = self.index.uid_unchecked(elem_ix);
            if elem.num_slots > 0 {
                let ix = elem.data_bucket_ix() as usize;
                let data_bucket = &self.data[ix];
                let loc = elem.data_loc(data_bucket);
                let data_bucket = &mut self.data[ix];
                //debug!(                    "DATA FREE {:?} {} {} {}",                    key, elem.data_location, data_bucket.capacity, elem_uid                );
                data_bucket.free(loc, elem_uid);
            }
            //debug!("INDEX FREE {:?} {}", key, elem_uid);
            self.index.free(elem_ix, elem_uid);
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
                    std::mem::size_of::<IndexEntry>() as u64,
                    // *2 causes rapid growth of index buckets
                    self.index.capacity_pow2 + i, // * 2,
                    self.index.max_search,
                    Arc::clone(&self.stats.index),
                    Arc::clone(&self.index.count),
                );
                let random = thread_rng().gen();
                let mut valid = true;
                for ix in 0..self.index.capacity() {
                    let uid = self.index.uid(ix);
                    if let Some(uid) = uid {
                        let elem: &IndexEntry = self.index.get(ix);
                        let new_ix =
                            Self::bucket_create_key(&mut index, &elem.key, uid, random, true);
                        if new_ix.is_err() {
                            valid = false;
                            break;
                        }
                        let new_ix = new_ix.unwrap();
                        let new_elem: &mut IndexEntry = index.get_mut(new_ix);
                        *new_elem = *elem;
                        /*
                        let dbg_elem: IndexEntry = *new_elem;
                        assert_eq!(
                            Self::bucket_find_entry(&index, &elem.key, random).unwrap(),
                            (&dbg_elem, new_ix)
                        );
                        */
                    }
                }
                if valid {
                    let sz = index.capacity();
                    {
                        let mut max = self.stats.index.max_size.lock().unwrap();
                        *max = std::cmp::max(*max, sz);
                    }
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

    pub fn apply_grow_index(&mut self, random: u64, index: BucketStorage) {
        self.random = random;
        self.index = index;
    }

    fn elem_size() -> u64 {
        std::mem::size_of::<T>() as u64
    }

    pub fn apply_grow_data(&mut self, ix: usize, bucket: BucketStorage) {
        if self.data.get(ix).is_none() {
            for i in self.data.len()..ix {
                // insert empty data buckets
                self.data.push(BucketStorage::new(
                    Arc::clone(&self.drives),
                    1 << i,
                    Self::elem_size(),
                    self.index.max_search,
                    Arc::clone(&self.stats.data),
                    Arc::default(),
                ))
            }
            self.data.push(bucket);
        } else {
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

    fn bucket_index_ix(index: &BucketStorage, key: &Pubkey, random: u64) -> u64 {
        let uid = IndexEntry::key_uid(key);
        let mut s = DefaultHasher::new();
        uid.hash(&mut s);
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
            let rv = self.try_write(key, new, refct);
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
