use {
    crate::{
        bucket_item::BucketItem,
        bucket_map::BucketMapError,
        bucket_stats::BucketMapStats,
        bucket_storage::{
            BucketCapacity, BucketOccupied, BucketStorage, Capacity, IncludeHeader,
            DEFAULT_CAPACITY_POW2,
        },
        index_entry::{
            DataBucket, IndexBucket, IndexEntry, IndexEntryPlaceInBucket, MultipleSlots,
            OccupiedEnum, OccupyIfMatches,
        },
        restart::RestartableBucket,
        MaxSearch, RefCount,
    },
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::hash_map::DefaultHasher,
        fs,
        hash::{Hash, Hasher},
        num::NonZeroU64,
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
    pub index: Option<BucketStorage<I>>,
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

/// when updating the index, this keeps track of the previous data entry which will need to be freed
struct DataFileEntryToFree {
    bucket_ix: usize,
    location: u64,
}

// >= 2 instances of BucketStorage per 'bucket' in the bucket map. 1 for index, >= 1 for data
pub struct Bucket<T: Copy + PartialEq + 'static> {
    drives: Arc<Vec<PathBuf>>,
    /// index
    pub index: BucketStorage<IndexBucket<T>>,
    /// random offset for the index
    random: u64,
    /// storage buckets to store SlotSlice up to a power of 2 in len
    pub data: Vec<BucketStorage<DataBucket>>,
    stats: Arc<BucketMapStats>,

    /// # entries caller expects the map to need to contain.
    /// Used as a hint for the next time we need to grow.
    anticipated_size: u64,

    pub reallocated: Reallocated<IndexBucket<T>, DataBucket>,

    /// set to true once any entries have been deleted from the index.
    /// Deletes indicate that there can be free slots and that the full search range must be searched for an entry.
    at_least_one_entry_deleted: bool,

    /// keep track of which index file this bucket is using so on restart we can try to reuse it
    restartable_bucket: RestartableBucket,

    /// true if this bucket was loaded (as opposed to created blank).
    /// When populating, we want to prioritize looking for data on disk that already matches as opposed to writing new data.
    #[allow(dead_code)]
    reused_file_at_startup: bool,
}

impl<'b, T: Clone + Copy + PartialEq + std::fmt::Debug + 'static> Bucket<T> {
    pub(crate) fn new(
        drives: Arc<Vec<PathBuf>>,
        max_search: MaxSearch,
        stats: Arc<BucketMapStats>,
        count: Arc<AtomicU64>,
        mut restartable_bucket: RestartableBucket,
    ) -> Self {
        let reuse_path = std::mem::take(&mut restartable_bucket.path);
        let elem_size = NonZeroU64::new(std::mem::size_of::<IndexEntry<T>>() as u64).unwrap();
        let (index, random, reused_file_at_startup) = reuse_path
            .and_then(|path| {
                // try to re-use the file this bucket was using last time we were running
                restartable_bucket.get().and_then(|(_file_name, random)| {
                    let result = BucketStorage::load_on_restart(
                        path.clone(),
                        elem_size,
                        max_search,
                        Arc::clone(&stats.index),
                        count.clone(),
                    )
                    .map(|index| (index, random, true /* true = reused file */));
                    if result.is_none() {
                        // we couldn't re-use it, so delete it
                        _ = fs::remove_file(path);
                    }
                    result
                })
            })
            .unwrap_or_else(|| {
                // no file to re-use, so create a new file
                let (index, file_name) = BucketStorage::new(
                    Arc::clone(&drives),
                    1,
                    elem_size.into(),
                    max_search,
                    Arc::clone(&stats.index),
                    count,
                );
                stats.index.resize_grow(0, index.capacity_bytes());
                let random = thread_rng().gen();
                restartable_bucket.set_file(file_name, random);
                (index, random, false /* true = reused file */)
            });

        Self {
            random,
            drives,
            index,
            data: vec![],
            stats,
            reallocated: Reallocated::default(),
            anticipated_size: 0,
            at_least_one_entry_deleted: false,
            restartable_bucket,
            reused_file_at_startup,
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
                let (v, ref_count) = ix.read_value(&self.index, &self.data);
                result.push(BucketItem {
                    pubkey: *key,
                    ref_count,
                    slot_list: v.to_vec(),
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
        index: &BucketStorage<IndexBucket<T>>,
        key: &Pubkey,
        random: u64,
    ) -> Result<(Option<IndexEntryPlaceInBucket<T>>, u64), BucketMapError> {
        let ix = Self::bucket_index_ix(key, random) % index.capacity();
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
            None => Err(BucketMapError::IndexNoSpace(index.contents.capacity())),
        }
    }

    fn bucket_find_index_entry(
        index: &BucketStorage<IndexBucket<T>>,
        key: &Pubkey,
        random: u64,
    ) -> Option<(IndexEntryPlaceInBucket<T>, u64)> {
        let ix = Self::bucket_index_ix(key, random) % index.capacity();
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
        let ix = Self::bucket_index_ix(key, random) % index.capacity();
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
        Err(BucketMapError::IndexNoSpace(index.contents.capacity()))
    }

    pub(crate) fn read_value(&self, key: &Pubkey) -> Option<(&[T], RefCount)> {
        //debug!("READ_VALUE: {:?}", key);
        let (elem, _) = self.find_index_entry(key)?;
        Some(elem.read_value(&self.index, &self.data))
    }

    /// for each item in `items`, get the hash value when hashed with `random`.
    /// Return a vec of tuples:
    /// (hash_value, index in `items`)
    fn index_entries(items: &[(Pubkey, T)], random: u64) -> Vec<(u64, usize)> {
        let mut inserts = Vec::with_capacity(items.len());
        items.iter().enumerate().for_each(|(i, (key, _v))| {
            let ix = Self::bucket_index_ix(key, random);
            inserts.push((ix, i));
        });
        inserts
    }

    /// batch insert of `items`. Assumption is a single slot list element and ref_count == 1.
    /// For any pubkeys that already exist, the index in `items` of the failed insertion and the existing data (previously put in the index) are returned.
    pub(crate) fn batch_insert_non_duplicates(&mut self, items: &[(Pubkey, T)]) -> Vec<(usize, T)> {
        assert!(
            !self.at_least_one_entry_deleted,
            "efficient batch insertion can only occur prior to any deletes"
        );
        let current_len = self.index.count.load(Ordering::Relaxed);
        let anticipated = items.len() as u64;
        self.set_anticipated_count((anticipated).saturating_add(current_len));
        let mut entries = Self::index_entries(items, self.random);
        let mut duplicates = Vec::default();
        let mut entries_created_on_disk = 0;
        // insert, but resizes may be necessary
        loop {
            let cap = self.index.capacity();
            // sort entries by their index % cap, so we'll search over the same spots in the file close to each other
            // `reverse()` is so we can efficiently pop off the end but get ascending order index values
            // sort before calling to make `batch_insert_non_duplicates_internal` easier to test.
            entries.sort_unstable_by(|a, b| (a.0 % cap).cmp(&(b.0 % cap)).reverse());

            let result = Self::batch_insert_non_duplicates_internal(
                &mut self.index,
                &self.data,
                items,
                &mut entries,
                &mut entries_created_on_disk,
                &mut duplicates,
                self.reused_file_at_startup,
            );
            match result {
                Ok(_result) => {
                    // everything added
                    self.set_anticipated_count(0);
                    self.index.count.fetch_add(
                        items.len().saturating_sub(duplicates.len()) as u64,
                        Ordering::Relaxed,
                    );
                    let stats = &self.index.stats.startup;
                    stats.entries_reused.fetch_add(
                        items
                            .len()
                            .saturating_sub(duplicates.len())
                            .saturating_sub(entries_created_on_disk) as u64,
                        Ordering::Relaxed,
                    );
                    stats
                        .entries_created
                        .fetch_add(entries_created_on_disk as u64, Ordering::Relaxed);
                    return duplicates;
                }
                Err(error) => {
                    // resize and add more
                    // `entries` will have had items removed from it
                    self.grow(error);
                    self.handle_delayed_grows();
                }
            }
        }
    }

    /// insert every entry in `reverse_sorted_entries` into the index as long as we can find a location where the data in the index
    /// file already matches the data we want to insert for the pubkey.
    /// for every entry that already exists in `index`, add it (and the value already in the index) to `duplicates`
    /// `reverse_sorted_entries` is (raw index (range = U64::MAX) in hash map, index in `items`)
    /// Any entries where the disk couldn't be updated are returned in `reverse_sorted_entries` or `duplicates`.
    /// The remaining items in `reverse_sorted_entries` can be inserted by over-writing non-matchingnew data to the index file.
    pub fn batch_insert_non_duplicates_reusing_file(
        index: &mut BucketStorage<IndexBucket<T>>,
        data_buckets: &[BucketStorage<DataBucket>],
        items: &[(Pubkey, T)],
        reverse_sorted_entries: &mut Vec<(u64, usize)>,
        duplicates: &mut Vec<(usize, T)>,
    ) {
        let max_search = index.max_search();
        let cap = index.capacity();
        let search_end = max_search.min(cap);
        let mut not_found = Vec::default();
        // pop one entry at a time to insert
        'outer: while let Some((ix_entry_raw, ix)) = reverse_sorted_entries.pop() {
            let (k, v) = &items[ix];
            // search for an empty spot starting at `ix_entry`
            for search in 0..search_end {
                let ix_index = (ix_entry_raw + search) % cap;
                let elem = IndexEntryPlaceInBucket::new(ix_index);
                match elem.occupy_if_matches(index, v, k) {
                    OccupyIfMatches::SuccessfulInit => {}
                    OccupyIfMatches::FoundDuplicate => {
                        // pubkey is same, and it is occupied, so we found a duplicate
                        let (v_existing, _ref_count_existing) =
                            elem.read_value(index, data_buckets);
                        // someone is already allocated with this pubkey, so we found a duplicate
                        duplicates.push((ix, *v_existing.first().unwrap()));
                    }
                    OccupyIfMatches::PubkeyMismatch => {
                        // fall through and look at next search value
                        continue;
                    }
                }
                continue 'outer; // this 'insertion' is completed - either because we found a duplicate or we occupied an entry in the file
            }
            // this pubkey did not exist in the file already and we exhausted the search space, so have to try the old way
            not_found.push((ix_entry_raw, ix));
        }
        // now add all entries that were not found
        // they were pushed in order since we popped off input
        // So, to keep them 'reversed', we need to reverse them here.
        // This isn't required for correctness, but fits the optimal iteration order.
        *reverse_sorted_entries = not_found.into_iter().rev().collect();
    }

    /// insert as much of `entries` as possible into `index`.
    /// return an error if the index needs to resize.
    /// for every entry that already exists in `index`, add it (and the value already in the index) to `duplicates`
    /// `reverse_sorted_entries` is (raw index (range = U64::MAX) in hash map, index in `items`)
    pub fn batch_insert_non_duplicates_internal(
        index: &mut BucketStorage<IndexBucket<T>>,
        data_buckets: &[BucketStorage<DataBucket>],
        items: &[(Pubkey, T)],
        reverse_sorted_entries: &mut Vec<(u64, usize)>,
        entries_created_on_disk: &mut usize,
        duplicates: &mut Vec<(usize, T)>,
        try_to_reuse_disk_data: bool,
    ) -> Result<(), BucketMapError> {
        if try_to_reuse_disk_data {
            // First, insert everything we can into disk contents that already have the right pubkey and hopefully the right data.
            // Ideally this results in no disk updates to insert these entries.
            // Any entries that were unable to be inserted would remain in `reverse_sorted_entries` so that we fall through and insert those
            // in any free slot we find.
            Self::batch_insert_non_duplicates_reusing_file(
                index,
                data_buckets,
                items,
                reverse_sorted_entries,
                duplicates,
            );
        }
        let max_search = index.max_search();
        let cap = index.capacity();
        let search_end = max_search.min(cap);

        // pop one entry at a time to insert in the first free location we find
        'outer: while let Some((ix_entry_raw, i)) = reverse_sorted_entries.pop() {
            let (k, v) = &items[i];
            // search for an empty spot starting at `ix_entry`
            for search in 0..search_end {
                let ix_index = (ix_entry_raw + search) % cap;
                let elem = IndexEntryPlaceInBucket::new(ix_index);
                if index.try_lock(ix_index) {
                    *entries_created_on_disk += 1;
                    // found free element and occupied it.
                    // Note that since we are in the startup phase where we only add and do not remove, it is NOT possible to find this same pubkey AFTER
                    //  the index we started searching at, or we would have found it as occupied BEFORE we were able to lock it here.
                    //  This precondition is not true once we are able to delete entries.

                    // These fields will be overwritten after allocation by callers.
                    // Since this part of the mmapped file could have previously been used by someone else, there can be garbage here.
                    elem.init(index, k);

                    // new data stored should be stored in IndexEntry and NOT in data file
                    // new data len is 1
                    elem.set_slot_count_enum_value(index, OccupiedEnum::OneSlotInIndex(v));
                    continue 'outer; // this 'insertion' is completed: inserted successfully
                } else {
                    // occupied, see if the key already exists here
                    if elem.key(index) == k {
                        let (v_existing, _ref_count_existing) =
                            elem.read_value(index, data_buckets);
                        duplicates.push((i, *v_existing.first().unwrap()));
                        continue 'outer; // this 'insertion' is completed: found a duplicate entry
                    }
                }
            }
            // search loop ended without finding a spot to insert this key
            // so, remember the item we were trying to insert for next time after resizing
            reverse_sorted_entries.push((ix_entry_raw, i));
            return Err(BucketMapError::IndexNoSpace(cap));
        }

        Ok(())
    }

    pub fn try_write(
        &mut self,
        key: &Pubkey,
        mut data: impl Iterator<Item = &'b T>,
        data_len: usize,
        ref_count: RefCount,
    ) -> Result<(), BucketMapError> {
        let num_slots = data_len as u64;
        let best_fit_bucket = MultipleSlots::data_bucket_from_num_slots(data_len as u64);
        let requires_data_bucket = num_slots > 1 || ref_count != 1;
        if requires_data_bucket && self.data.get(best_fit_bucket as usize).is_none() {
            // fail early if the data bucket we need doesn't exist - we don't want the index entry partially allocated
            return Err(BucketMapError::DataNoSpace((best_fit_bucket, 0)));
        }
        let max_search = self.index.max_search();
        let (elem, elem_ix) = Self::find_index_entry_mut(&self.index, key, self.random)?;
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
        if !requires_data_bucket {
            // new data stored should be stored in IndexEntry and NOT in data file
            // new data len is 0 or 1
            if let OccupiedEnum::MultipleSlots(multiple_slots) =
                elem.get_slot_count_enum(&self.index)
            {
                let bucket_ix = multiple_slots.data_bucket_ix() as usize;
                // free the entry in the data bucket the data was previously stored in
                let loc = multiple_slots.data_loc(&self.data[bucket_ix]);
                self.data[bucket_ix].free(loc);
            }
            elem.set_slot_count_enum_value(
                &mut self.index,
                if let Some(single_element) = data.next() {
                    OccupiedEnum::OneSlotInIndex(single_element)
                } else {
                    OccupiedEnum::ZeroSlots
                },
            );
            return Ok(());
        }

        // storing the slot list requires using the data file
        let mut old_data_entry_to_free = None;
        // see if old elements were in a data file
        if let Some(multiple_slots) = elem.get_multiple_slots_mut(&mut self.index) {
            let bucket_ix = multiple_slots.data_bucket_ix() as usize;
            let current_bucket = &mut self.data[bucket_ix];
            let elem_loc = multiple_slots.data_loc(current_bucket);

            if best_fit_bucket == bucket_ix as u64 {
                // in place update in same data file
                MultipleSlots::set_ref_count(current_bucket, elem_loc, ref_count);

                // write data
                assert!(!current_bucket.is_free(elem_loc));
                let slice: &mut [T] = current_bucket.get_slice_mut(
                    elem_loc,
                    data_len as u64,
                    IncludeHeader::NoHeader,
                );
                multiple_slots.set_num_slots(num_slots);

                slice.iter_mut().zip(data).for_each(|(dest, src)| {
                    *dest = *src;
                });
                return Ok(());
            }

            // not updating in place, so remember old entry to free
            // Wait to free until we make sure we don't have to resize the best_fit_bucket
            old_data_entry_to_free = Some(DataFileEntryToFree {
                bucket_ix,
                location: elem_loc,
            });
        }

        // need to move the allocation to a best fit spot
        let best_bucket = &mut self.data[best_fit_bucket as usize];
        let cap_power = best_bucket.contents.capacity_pow2();
        let cap = best_bucket.capacity();
        let pos = thread_rng().gen_range(0..cap);
        let mut success = false;
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
                let mut multiple_slots = MultipleSlots::default();
                multiple_slots.set_storage_offset(ix);
                multiple_slots
                    .set_storage_capacity_when_created_pow2(best_bucket.contents.capacity_pow2());
                multiple_slots.set_num_slots(num_slots);
                MultipleSlots::set_ref_count(best_bucket, ix, ref_count);

                //debug!(                        "DATA ALLOC {:?} {} {} {}",                        key, elem.data_location, best_bucket.capacity, elem_uid                    );
                let best_bucket = &mut self.data[best_fit_bucket as usize];
                best_bucket.occupy(ix, false).unwrap();
                if num_slots > 0 {
                    // copy slotlist into the data bucket
                    let slice = best_bucket.get_slice_mut(ix, num_slots, IncludeHeader::NoHeader);
                    slice.iter_mut().zip(data).for_each(|(dest, src)| {
                        *dest = *src;
                    });
                }

                // update index bucket after data bucket has been updated.
                elem.set_slot_count_enum_value(
                    &mut self.index,
                    OccupiedEnum::MultipleSlots(&multiple_slots),
                );
                success = true;
                break;
            }
        }
        if !success {
            return Err(BucketMapError::DataNoSpace((best_fit_bucket, cap_power)));
        }
        if let Some(DataFileEntryToFree {
            bucket_ix,
            location,
        }) = old_data_entry_to_free
        {
            // free the entry in the data bucket the data was previously stored in
            self.data[bucket_ix].free(location);
        }
        Ok(())
    }

    pub fn delete_key(&mut self, key: &Pubkey) {
        if let Some((elem, elem_ix)) = self.find_index_entry(key) {
            self.at_least_one_entry_deleted = true;
            if let OccupiedEnum::MultipleSlots(multiple_slots) =
                elem.get_slot_count_enum(&self.index)
            {
                let ix = multiple_slots.data_bucket_ix() as usize;
                let data_bucket = &self.data[ix];
                let loc = multiple_slots.data_loc(data_bucket);
                let data_bucket = &mut self.data[ix];
                //debug!(                    "DATA FREE {:?} {} {} {}",                    key, elem.data_location, data_bucket.capacity, elem_uid                );
                data_bucket.free(loc);
            }
            //debug!("INDEX FREE {:?} {}", key, elem_uid);
            self.index.free(elem_ix);
        }
    }

    pub(crate) fn set_anticipated_count(&mut self, count: u64) {
        self.anticipated_size = count;
    }

    pub fn grow_index(&self, mut current_capacity: u64) {
        if self.index.contents.capacity() == current_capacity {
            // make sure to grow to at least % more than the anticipated size
            // The indexing algorithm expects to require some over-allocation.
            let anticipated_size = self.anticipated_size * 140 / 100;
            let mut m = Measure::start("grow_index");
            //debug!("GROW_INDEX: {}", current_capacity_pow2);
            let mut count = 0;
            loop {
                count += 1;
                // grow relative to the current capacity
                let new_capacity = (current_capacity * 110 / 100).max(anticipated_size);
                let (mut index, file_name) = BucketStorage::new_with_capacity(
                    Arc::clone(&self.drives),
                    1,
                    std::mem::size_of::<IndexEntry<T>>() as u64,
                    Capacity::Actual(new_capacity),
                    self.index.max_search,
                    Arc::clone(&self.stats.index),
                    Arc::clone(&self.index.count),
                );
                // index may have allocated something larger than we asked for,
                // so, in case we fail to reindex into this larger size, grow from this size next iteration.
                current_capacity = index.capacity();
                let mut valid = true;
                for ix in 0..self.index.capacity() {
                    if !self.index.is_free(ix) {
                        let elem: &IndexEntry<T> = self.index.get(ix);
                        let new_ix =
                            Self::bucket_create_key(&mut index, &elem.key, self.random, true);
                        if new_ix.is_err() {
                            valid = false;
                            break;
                        }
                        let new_ix = new_ix.unwrap();
                        let new_elem: &mut IndexEntry<T> = index.get_mut(new_ix);
                        *new_elem = *elem;
                        index.copying_entry(new_ix, &self.index, ix);
                    }
                }
                if valid {
                    self.stats.index.update_max_size(index.capacity());
                    let mut items = self.reallocated.items.lock().unwrap();
                    items.index = Some(index);
                    self.reallocated.add_reallocation();
                    self.restartable_bucket.set_file(file_name, self.random);
                    break;
                }
            }
            m.stop();
            if count > 1 {
                self.stats
                    .index
                    .failed_resizes
                    .fetch_add(count - 1, Ordering::Relaxed);
            }
            self.stats.index.resizes.fetch_add(1, Ordering::Relaxed);
            self.stats
                .index
                .resize_us
                .fetch_add(m.as_us(), Ordering::Relaxed);
        }
    }

    pub fn apply_grow_index(&mut self, mut index: BucketStorage<IndexBucket<T>>) {
        self.stats
            .index
            .resize_grow(self.index.capacity_bytes(), index.capacity_bytes());

        if self.restartable_bucket.restart.is_some() {
            // we are keeping track of which files we use for restart.
            // And we are resizing.
            // So, delete the old file and set the new file to NOT delete.
            // This way the new file will still be around on startup.
            // We are completely done with the old file.
            self.index.delete_file_on_drop = true;
            index.delete_file_on_drop = false;
        }
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
                self.add_data_bucket(
                    BucketStorage::new(
                        Arc::clone(&self.drives),
                        1 << i,
                        Self::elem_size(),
                        self.index.max_search,
                        Arc::clone(&self.stats.data),
                        Arc::default(),
                    )
                    .0,
                );
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
        let (new_bucket, _file_name) = BucketStorage::new_resized(
            &self.drives,
            self.index.max_search,
            self.data.get(data_index as usize),
            Capacity::Pow2(std::cmp::max(
                current_capacity_pow2 + 1,
                DEFAULT_CAPACITY_POW2,
            )),
            1 << data_index,
            Self::elem_size(),
            &self.stats.data,
        );
        self.reallocated.add_reallocation();
        let mut items = self.reallocated.items.lock().unwrap();
        items.data = Some((data_index, new_bucket));
    }

    fn bucket_index_ix(key: &Pubkey, random: u64) -> u64 {
        let mut s = DefaultHasher::new();
        key.hash(&mut s);
        //the locally generated random will make it hard for an attacker
        //to deterministically cause all the pubkeys to land in the same
        //location in any bucket on all validators
        random.hash(&mut s);
        s.finish()
        //debug!(            "INDEX_IX: {:?} uid:{} loc: {} cap:{}",            key,            uid,            location,            index.capacity()        );
    }

    /// grow the appropriate piece. Note this takes an immutable ref.
    /// The actual grow is set into self.reallocated and applied later on a write lock
    pub(crate) fn grow(&self, err: BucketMapError) {
        match err {
            BucketMapError::DataNoSpace((data_index, current_capacity_pow2)) => {
                //debug!("GROWING SPACE {:?}", (data_index, current_capacity_pow2));
                self.grow_data(data_index, current_capacity_pow2);
            }
            BucketMapError::IndexNoSpace(current_capacity) => {
                //debug!("GROWING INDEX {}", sz);
                self.grow_index(current_capacity);
            }
        }
    }

    /// if a bucket was resized previously with a read lock, then apply that resize now
    pub fn handle_delayed_grows(&mut self) {
        if self.reallocated.get_reallocated() {
            // swap out the bucket that was resized previously with a read lock
            let mut items = std::mem::take(&mut *self.reallocated.items.lock().unwrap());

            if let Some(bucket) = items.index.take() {
                self.apply_grow_index(bucket);
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
            let Err(err) = self.try_write(key, new.iter(), new.len(), refct) else {
                return;
            };
            self.grow(err);
            self.handle_delayed_grows();
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

#[cfg(test)]
mod tests {
    use {super::*, crate::index_entry::OccupyIfMatches, tempfile::tempdir};

    #[test]
    fn test_batch_insert_non_duplicates_reusing_file_many_entries() {
        // 3 variations of reuse
        for reuse_type in 0..3 {
            let data_buckets = Vec::default();
            let v = 12u64;
            let random = 1;
            // with random=1, 6 entries is the most that don't collide on a single hash % cap value.
            for len in 0..7 {
                log::error!("testing with {len}");
                // cannot use pubkey [0,0,...] because that matches a zeroed out default file contents.
                let raw = (0..len)
                    .map(|l| (Pubkey::from([(l + 1) as u8; 32]), v + (l as u64)))
                    .collect::<Vec<_>>();

                let mut hashed = Bucket::index_entries(&raw, random);
                let hashed_raw = hashed.clone();

                let tmpdir = tempdir().unwrap();
                let paths: Arc<Vec<PathBuf>> = Arc::new(vec![tmpdir.path().to_path_buf()]);
                assert!(!paths.is_empty());
                let max_search = 2;
                let (mut index, file_name) = BucketStorage::<IndexBucket<u64>>::new(
                    paths.clone(),
                    1,
                    std::mem::size_of::<crate::index_entry::IndexEntry<u64>>() as u64,
                    max_search,
                    Arc::default(),
                    Arc::default(),
                );
                index.delete_file_on_drop = false;
                let cap = index.capacity();

                hashed.sort_unstable_by(|a, b| (a.0 % cap).cmp(&(b.0 % cap)).reverse());
                hashed.windows(2).for_each(|two| {
                    assert_ne!(two[0].0 % cap, two[1].0 % cap, "{two:?}, cap: {cap}");
                });

                // file is blank, so nothing matches, so everything returned in `hashed` to retry.
                let mut duplicates = Vec::default();
                let mut entries_created = 0;
                // insert normally
                Bucket::<u64>::batch_insert_non_duplicates_internal(
                    &mut index,
                    &data_buckets,
                    &raw,
                    &mut hashed,
                    &mut entries_created,
                    &mut duplicates,
                    false,
                )
                .unwrap();
                assert!(hashed.is_empty());
                assert!(duplicates.is_empty());

                hashed_raw.iter().for_each(|(hash, i)| {
                    let (k, v) = raw[*i];
                    let ix = *hash % cap;
                    let entry = IndexEntryPlaceInBucket::new(ix);
                    assert_eq!(entry.key(&index), &k);
                    assert_eq!(
                        entry.get_slot_count_enum(&index),
                        OccupiedEnum::OneSlotInIndex(&v)
                    );
                });

                drop(index);
                let path = paths.first().unwrap().join(file_name.to_string());
                let mut index = BucketStorage::<IndexBucket<u64>>::load_on_restart(
                    path,
                    NonZeroU64::new(
                        std::mem::size_of::<crate::index_entry::IndexEntry<u64>>() as u64
                    )
                    .unwrap(),
                    max_search,
                    Arc::default(),
                    Arc::default(),
                )
                .unwrap();

                // verify index file is unoccupied, but that contents match
                hashed_raw.iter().for_each(|(hash, i)| {
                    let (k, _v) = raw[*i];
                    let ix = *hash % cap;
                    let entry = IndexEntryPlaceInBucket::new(ix);
                    assert_eq!(entry.key(&index), &k);
                    assert_eq!(entry.get_slot_count_enum(&index), OccupiedEnum::Free);
                });

                // this was wiped out by the last call to batch_insert..., so recreate it.
                hashed = hashed_raw.clone();
                let mut duplicates = Vec::default();
                if reuse_type == 0 {
                    Bucket::<u64>::batch_insert_non_duplicates_reusing_file(
                        &mut index,
                        &data_buckets,
                        &raw,
                        &mut hashed,
                        &mut duplicates,
                    );
                } else if reuse_type == 1 {
                    // just overwrite all data instead of trying to reuse it
                    let mut entries_created = 0;
                    _ = Bucket::<u64>::batch_insert_non_duplicates_internal(
                        &mut index,
                        &data_buckets,
                        &raw,
                        &mut hashed,
                        &mut entries_created,
                        &mut duplicates,
                        false,
                    );
                    assert_eq!(entries_created, hashed_raw.len());
                } else if reuse_type == 2 {
                    // call the higher level fn
                    // That fn will call batch_insert_non_duplicates_reusing_file.
                    // The inner fn should insert everything, reusing data, so there should be no entries created.
                    let mut entries_created = 0;
                    _ = Bucket::<u64>::batch_insert_non_duplicates_internal(
                        &mut index,
                        &data_buckets,
                        &raw,
                        &mut hashed,
                        &mut entries_created,
                        &mut duplicates,
                        // call re-use code first
                        true,
                    );
                    assert_eq!(entries_created, 0);
                }
                assert!(hashed.is_empty());
                assert!(duplicates.is_empty());

                hashed_raw.iter().for_each(|(hash, i)| {
                    let (k, v) = raw[*i];
                    let ix = *hash % cap;
                    let entry = IndexEntryPlaceInBucket::new(ix);
                    assert_eq!(entry.key(&index), &k);
                    assert_eq!(
                        entry.get_slot_count_enum(&index),
                        OccupiedEnum::OneSlotInIndex(&v),
                        "i: {i}"
                    );
                });
            }
        }
    }

    #[test]
    fn test_batch_insert_non_duplicates_reusing_file_blank_file() {
        let data_buckets = Vec::default();
        let v = 12u64;
        let random = 1;
        for len in 1..4 {
            // cannot use pubkey [0,0,...] because that matches a zeroed out default file contents.
            let raw = (0..len)
                .map(|l| (Pubkey::from([(l + 1) as u8; 32]), v + (l as u64)))
                .collect::<Vec<_>>();

            let mut hashed = Bucket::index_entries(&raw, random);
            let hashed_raw = hashed.clone();

            let mut index = create_test_index(None);

            let cap = index.capacity();
            let ix = hashed[0].0 % cap;

            let entry = IndexEntryPlaceInBucket::new(ix);

            // file is blank, so nothing matches, so everything returned in `hashed` to retry.
            let mut duplicates = Vec::default();
            Bucket::<u64>::batch_insert_non_duplicates_reusing_file(
                &mut index,
                &data_buckets,
                &raw,
                &mut hashed,
                &mut duplicates,
            );

            assert_eq!(entry.get_slot_count_enum(&index), OccupiedEnum::Free);
            assert_eq!(entry.key(&index), &Pubkey::default());
            assert_eq!(hashed, hashed_raw, "len: {len}");
            assert!(duplicates.is_empty());
        }
    }

    #[should_panic(expected = "index asked to insert the same data twice")]
    #[test]
    fn test_batch_insert_non_duplicates_reusing_file_insert_twice() {
        let data_buckets = Vec::default();
        let v = 12u64;
        let random = 1;
        // cannot use pubkey [0,0,...] because that matches a zeroed out default file contents.
        let len = 1;
        let raw = (0..len)
            .map(|l| (Pubkey::from([(l + 1) as u8; 32]), v + (l as u64)))
            .collect::<Vec<_>>();

        let mut hashed = Bucket::index_entries(&raw, random);

        let mut index = create_test_index(None);
        let cap = index.capacity();
        let ix = hashed[0].0 % cap;
        let entry = IndexEntryPlaceInBucket::new(ix);
        entry.init(&mut index, &raw[0].0);
        entry.set_slot_count_enum_value(&mut index, OccupiedEnum::OneSlotInIndex(&raw[0].1));

        let mut duplicates = Vec::default();
        // this will assert because the same k,v pair are already occupied in the index.
        Bucket::<u64>::batch_insert_non_duplicates_reusing_file(
            &mut index,
            &data_buckets,
            &raw,
            &mut hashed,
            &mut duplicates,
        );
    }

    #[test]
    fn test_batch_insert_non_duplicates_reusing_file_insert_duplicate() {
        let data_buckets = Vec::default();
        let v = 12u64;
        let random = 1;
        // cannot use pubkey [0,0,...] because that matches a zeroed out default file contents.
        let len = 1;
        let raw = (0..len)
            .map(|l| (Pubkey::from([(l + 1) as u8; 32]), v + (l as u64)))
            .collect::<Vec<_>>();

        let mut hashed = Bucket::index_entries(&raw, random);

        let mut index = create_test_index(None);
        let cap = index.capacity();
        let ix = hashed[0].0 % cap;

        // occupy the index data entry with same pubkey, different value.
        // This causes it to be treated as a duplicate.
        let entry = IndexEntryPlaceInBucket::new(ix);
        entry.init(&mut index, &(raw[0].0));
        let non_matching_v = raw[0].1 + 1;
        entry.set_slot_count_enum_value(&mut index, OccupiedEnum::OneSlotInIndex(&non_matching_v));

        // since the same key is already in use with a different value, it is a duplicate
        let mut duplicates = Vec::default();
        Bucket::<u64>::batch_insert_non_duplicates_reusing_file(
            &mut index,
            &data_buckets,
            &raw,
            &mut hashed,
            &mut duplicates,
        );
        assert_eq!(
            entry.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&non_matching_v)
        );

        assert!(hashed.is_empty());
        assert_eq!(duplicates, vec![(0, non_matching_v)], "len: {len}");
    }

    #[test]
    fn test_batch_insert_non_duplicates_reusing_file_skip_one() {
        let data_buckets = Vec::default();
        let v = 12u64;
        let random = 1;
        // cannot use pubkey [0,0,...] because that matches a zeroed out default file contents.
        let len = 1;
        let mut raw = (0..len + 1)
            .map(|l| (Pubkey::from([(l + 1) as u8; 32]), v + (l as u64)))
            .collect::<Vec<_>>();

        let other = raw.pop().unwrap();
        let mut hashed = Bucket::index_entries(&raw, random);

        let mut index = create_test_index(None);
        let cap = index.capacity();
        let ix = hashed[0].0 % cap;

        // occupy the index data entry with a different pubkey
        // This causes it to be skipped.
        let entry = IndexEntryPlaceInBucket::new(ix);
        entry.init(&mut index, &(other.0));
        let entry = IndexEntryPlaceInBucket::new(ix + 1);
        // sets pubkey value and enum value of ZeroSlots. Leaving it at zero causes issues.
        entry.init(&mut index, &(raw[0].0));
        // marks as free but does not clear out pubkey data in the file. This simulates finding the correct pubkey in the data file in a free entry and occupying it.
        entry.set_slot_count_enum_value(&mut index, OccupiedEnum::Free);

        // since the same key is already in use with a different value, it is a duplicate
        let mut duplicates = Vec::default();
        Bucket::<u64>::batch_insert_non_duplicates_reusing_file(
            &mut index,
            &data_buckets,
            &raw,
            &mut hashed,
            &mut duplicates,
        );

        assert_eq!(
            entry.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&raw[0].1)
        );

        assert!(hashed.is_empty());
        assert!(duplicates.is_empty());

        let entry = IndexEntryPlaceInBucket::new(ix);
        assert_eq!(entry.key(&index), &other.0);
        let entry = IndexEntryPlaceInBucket::new(ix + 1);
        assert_eq!(entry.key(&index), &raw[0].0);
    }

    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    #[test]
    fn test_batch_insert_non_duplicates_reusing_file_existing_zero() {
        let data_buckets = Vec::default();
        let v = 12u64;
        let random = 1;
        // cannot use pubkey [0,0,...] because that matches a zeroed out default file contents.
        let len = 1;
        let mut raw = (0..len + 1)
            .map(|l| (Pubkey::from([(l + 1) as u8; 32]), v + (l as u64)))
            .collect::<Vec<_>>();

        let other = raw.pop().unwrap();
        let mut hashed = Bucket::index_entries(&raw, random);

        let mut index = create_test_index(None);
        let cap = index.capacity();
        let ix = hashed[0].0 % cap;

        // occupy the index data entry with a different pubkey
        // This causes it to be skipped.
        let entry = IndexEntryPlaceInBucket::new(ix);
        entry.init(&mut index, &(other.0));
        let entry = IndexEntryPlaceInBucket::new(ix + 1);
        // sets pubkey value and enum value of ZeroSlots. Leaving it at zero is illegal at startup, so we'll assert when we find this duplicate.
        entry.init(&mut index, &(raw[0].0));

        // since the same key is already in use with a different value, it is a duplicate.
        // But, it is a zero length entry. This is not supported at startup. Startup would have never generated a zero length occupied entry.
        // So, it is ok for this to assert.
        let mut duplicates = Vec::default();
        Bucket::<u64>::batch_insert_non_duplicates_reusing_file(
            &mut index,
            &data_buckets,
            &raw,
            &mut hashed,
            &mut duplicates,
        );
    }

    #[test]
    fn test_index_entries() {
        for v in 10..12u64 {
            for random in 1..3 {
                for len in 1..3 {
                    let raw = (0..len)
                        .map(|l| {
                            let k = Pubkey::from([l as u8; 32]);
                            (k, v + (l as u64))
                        })
                        .collect::<Vec<_>>();
                    let hashed = Bucket::index_entries(&raw, random);
                    assert_eq!(hashed.len(), len);
                    (0..len).for_each(|i| {
                        let raw = raw[i];
                        let hashed = hashed[i];
                        assert_eq!(Bucket::<u64>::bucket_index_ix(&raw.0, random), hashed.0);
                        assert_eq!(i, hashed.1);
                    });
                }
            }
        }
    }

    fn create_test_index(max_search: Option<u8>) -> BucketStorage<IndexBucket<u64>> {
        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());
        let max_search = max_search.unwrap_or(2);
        BucketStorage::<IndexBucket<u64>>::new(
            Arc::new(paths),
            1,
            std::mem::size_of::<crate::index_entry::IndexEntry<u64>>() as u64,
            max_search,
            Arc::default(),
            Arc::default(),
        )
        .0
    }

    #[test]
    fn batch_insert_duplicates_internal_simple() {
        for try_to_reuse_disk_data in [false, true] {
            // add the same duplicate key several times.
            // make sure the resulting index and returned `duplicates` is correct.
            let random = 1;
            let data_buckets = Vec::default();
            let k = Pubkey::from([1u8; 32]);
            for v in 10..12u64 {
                for len in 1..4 {
                    let raw = (0..len).map(|l| (k, v + (l as u64))).collect::<Vec<_>>();
                    let mut hashed = Bucket::index_entries(&raw, random);
                    let hashed_raw = hashed.clone();

                    let mut index = create_test_index(None);

                    let mut entries_created = 0;
                    let mut duplicates = Vec::default();
                    assert!(Bucket::<u64>::batch_insert_non_duplicates_internal(
                        &mut index,
                        &Vec::default(),
                        &raw,
                        &mut hashed,
                        &mut entries_created,
                        &mut duplicates,
                        try_to_reuse_disk_data,
                    )
                    .is_ok());

                    assert_eq!(duplicates.len(), len as usize - 1);
                    assert_eq!(hashed.len(), 0);
                    let single_hashed_raw_inserted = hashed_raw.last().unwrap();
                    let elem = IndexEntryPlaceInBucket::new(
                        single_hashed_raw_inserted.0 % index.capacity(),
                    );
                    let (value, ref_count) = elem.read_value(&index, &data_buckets);
                    assert_eq!(ref_count, 1);
                    assert_eq!(value, &[raw[single_hashed_raw_inserted.1].1]);
                    let expected_duplicates = hashed_raw
                        .iter()
                        .rev()
                        .skip(1)
                        .map(|(_hash, i)| (*i, raw[single_hashed_raw_inserted.1].1))
                        .collect::<Vec<_>>();
                    assert_eq!(expected_duplicates, duplicates);
                }
            }
        }
    }

    #[test]
    fn batch_insert_non_duplicates_internal_simple() {
        for try_to_reuse_disk_data in [false, true] {
            // add 2 entries, make sure they are added in the buckets we expect
            let random = 1;
            let data_buckets = Vec::default();
            for v in 10..12u64 {
                for len in 1..3 {
                    let raw = (0..len)
                        .map(|l| {
                            let k = Pubkey::from([l as u8; 32]);
                            (k, v + (l as u64))
                        })
                        .collect::<Vec<_>>();
                    let mut hashed = Bucket::index_entries(&raw, random);
                    let hashed_raw = hashed.clone();

                    let mut index = create_test_index(None);

                    let mut duplicates = Vec::default();
                    let mut entries_created = 0;
                    assert!(Bucket::<u64>::batch_insert_non_duplicates_internal(
                        &mut index,
                        &Vec::default(),
                        &raw,
                        &mut hashed,
                        &mut entries_created,
                        &mut duplicates,
                        try_to_reuse_disk_data,
                    )
                    .is_ok());

                    assert_eq!(hashed.len(), 0);
                    (0..len).for_each(|i| {
                        let raw2 = hashed_raw[i];
                        let elem = IndexEntryPlaceInBucket::new(raw2.0 % index.capacity());
                        let (value, ref_count) = elem.read_value(&index, &data_buckets);
                        assert_eq!(ref_count, 1);
                        assert_eq!(value, &[raw[hashed_raw[i].1].1]);
                    });
                }
            }
        }
    }

    #[test]
    fn batch_insert_non_duplicates_internal_same_ix_exceeds_max_search() {
        for try_to_reuse_disk_data in [false, true] {
            // add `len` entries with the same ix, make sure they are added in subsequent buckets.
            // adjust `max_search`. If we try to add an entry that causes us to exceed `max_search`, then assert that the adding fails with an error and
            // the colliding item remains in `entries`
            let random = 1;
            let data_buckets = Vec::default();
            for max_search in [2usize, 3] {
                for v in 10..12u64 {
                    for len in 1..(max_search + 1) {
                        let raw = (0..len)
                            .map(|l| {
                                // +1 because pubkey[0,0,...] matches default contents of index file
                                let k = Pubkey::from([(l + 1) as u8; 32]);
                                (k, v + (l as u64))
                            })
                            .collect::<Vec<_>>();
                        let mut hashed = Bucket::index_entries(&raw, random);
                        let common_ix = 2; // both are put at same ix
                        hashed.iter_mut().for_each(|v| {
                            v.0 = common_ix;
                        });
                        let hashed_raw = hashed.clone();

                        let mut index = create_test_index(Some(max_search as u8));

                        let mut duplicates = Vec::default();
                        let mut entries_created = 0;
                        let result = Bucket::<u64>::batch_insert_non_duplicates_internal(
                            &mut index,
                            &Vec::default(),
                            &raw,
                            &mut hashed,
                            &mut entries_created,
                            &mut duplicates,
                            try_to_reuse_disk_data,
                        );

                        assert_eq!(
                            hashed.len(),
                            if len > max_search { 1 } else { 0 },
                            "len: {len}"
                        );
                        (0..len).for_each(|i| {
                            assert!(if len > max_search {
                                result.is_err()
                            } else {
                                result.is_ok()
                            });
                            let raw2 = hashed_raw[i];
                            if i == 0 && len > max_search {
                                // max search was exceeded and the first entry was unable to be inserted, so it remained in `hashed`
                                assert_eq!(hashed[0], hashed_raw[0]);
                            } else {
                                // we insert in reverse order when ix values are equal, so we expect to find item[1] in item[1]'s expected ix and item[0] will be 1 search distance away from expected ix
                                let search_required = (len - i - 1) as u64;
                                let elem = IndexEntryPlaceInBucket::new(
                                    (raw2.0 + search_required) % index.capacity(),
                                );
                                let (value, ref_count) = elem.read_value(&index, &data_buckets);
                                assert_eq!(ref_count, 1);
                                assert_eq!(value, &[raw[hashed_raw[i].1].1]);
                            }
                        });
                    }
                }
            }
        }
    }

    #[test]
    fn test_occupy_if_matches() {
        let random = 1;
        let k = Pubkey::from([1u8; 32]);
        let k2 = Pubkey::from([2u8; 32]);
        let v = 12u64;
        let v2 = 13u64;
        let raw = vec![(k, v)];
        let hashed = Bucket::index_entries(&raw, random);
        let hashed_raw = hashed.clone();

        let mut index = create_test_index(None);

        let single_hashed_raw_inserted = hashed_raw.last().unwrap();
        let elem = IndexEntryPlaceInBucket::new(single_hashed_raw_inserted.0 % index.capacity());

        assert_eq!(elem.get_slot_count_enum(&index), OccupiedEnum::Free);
        elem.init(&mut index, &k);
        elem.set_slot_count_enum_value(&mut index, OccupiedEnum::OneSlotInIndex(&v));
        assert_eq!(
            elem.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&v)
        );
        // clear it
        elem.set_slot_count_enum_value(&mut index, OccupiedEnum::Free);
        assert_eq!(elem.get_slot_count_enum(&index), OccupiedEnum::Free);

        assert_eq!(
            elem.occupy_if_matches(&mut index, &v, &k),
            OccupyIfMatches::SuccessfulInit
        );
        assert_eq!(
            elem.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&v)
        );
        // clear it
        elem.set_slot_count_enum_value(&mut index, OccupiedEnum::Free);
        assert_eq!(elem.get_slot_count_enum(&index), OccupiedEnum::Free);

        // v2 but will still write it
        assert_eq!(
            elem.occupy_if_matches(&mut index, &v2, &k),
            OccupyIfMatches::SuccessfulInit
        );
        assert_eq!(
            elem.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&v2)
        );

        // already a different occupied value for this pubkey in the index, so found duplicate
        assert_eq!(
            elem.occupy_if_matches(&mut index, &v, &k),
            OccupyIfMatches::FoundDuplicate
        );
        assert_eq!(
            elem.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&v2)
        );

        // k2 is pubkey mismatch
        assert_eq!(
            elem.occupy_if_matches(&mut index, &v, &k2),
            OccupyIfMatches::PubkeyMismatch
        );

        // clear it
        elem.set_slot_count_enum_value(&mut index, OccupiedEnum::Free);
        assert_eq!(elem.get_slot_count_enum(&index), OccupiedEnum::Free);

        // k2 is pubkey mismatch
        assert_eq!(
            elem.occupy_if_matches(&mut index, &v, &k2),
            OccupyIfMatches::PubkeyMismatch
        );
    }

    #[test]
    #[should_panic(expected = "index asked to insert the same data twice")]
    fn test_occupy_if_matches_panic() {
        solana_logger::setup();
        let random = 1;
        let k = Pubkey::from([1u8; 32]);
        let v = 12u64;
        let raw = vec![(k, v)];
        let hashed = Bucket::index_entries(&raw, random);
        let hashed_raw = hashed.clone();

        let mut index = create_test_index(None);

        let single_hashed_raw_inserted = hashed_raw.last().unwrap();
        let elem = IndexEntryPlaceInBucket::new(single_hashed_raw_inserted.0 % index.capacity());

        assert_eq!(elem.get_slot_count_enum(&index), OccupiedEnum::Free);
        elem.init(&mut index, &k);
        elem.set_slot_count_enum_value(&mut index, OccupiedEnum::OneSlotInIndex(&v));
        assert_eq!(
            elem.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&v)
        );

        assert_eq!(
            elem.occupy_if_matches(&mut index, &v, &k),
            OccupyIfMatches::SuccessfulInit
        );
        assert_eq!(
            elem.get_slot_count_enum(&index),
            OccupiedEnum::OneSlotInIndex(&v)
        );
    }

    #[should_panic(expected = "batch insertion can only occur prior to any deletes")]
    #[test]
    fn batch_insert_after_delete() {
        solana_logger::setup();

        let tmpdir = tempdir().unwrap();
        let paths: Vec<PathBuf> = vec![tmpdir.path().to_path_buf()];
        assert!(!paths.is_empty());
        let max_search = 2;
        let mut bucket = Bucket::new(
            Arc::new(paths),
            max_search,
            Arc::default(),
            Arc::default(),
            RestartableBucket::default(),
        );

        let key = Pubkey::new_unique();
        assert_eq!(bucket.read_value(&key), None);

        bucket.update(&key, |_| Some((vec![0], 0)));
        bucket.delete_key(&key);

        bucket.batch_insert_non_duplicates(&[]);
    }
}
