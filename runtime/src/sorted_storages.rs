use {
    crate::accounts_db::AccountStorageEntry,
    log::*,
    solana_measure::measure,
    solana_sdk::clock::Slot,
    std::{
        ops::{Bound, Range, RangeBounds},
        sync::Arc,
    },
};

/// bprumo TODO: docs, talk about providing contiguous sorted-by-slot access
/// Provide access to SnapshotStorageOnes by slot
#[derive(Debug)]
pub struct SortedStorages<'a> {
    /// range of slots where storages exist (likely sparse)
    /// bprumo note: could be looked up with .first() and .last() in the array, but the .slot() is an atomic lookup in the appendvec, so just cache it here
    slots: Range<Slot>,
    /// the actual storages
    /// A HashMap allows sparse storage and fast lookup of Slot -> Storage.
    /// We expect ~432k slots.
    //storages: HashMap<Slot, &'a Arc<AccountStorageEntry>>,
    storages: Box<[(Slot, &'a Arc<AccountStorageEntry>)]>,
}

impl<'a> SortedStorages<'a> {
    /// bprumo TODO: doc
    pub fn new(source: &'a [Arc<AccountStorageEntry>]) -> Self {
        let source_with_slots = source.into_iter().map(|storage| (storage.slot(), storage));
        Self::new_with_slots(source_with_slots, None, None)
    }

    /// bprumo TODO: doc
    pub fn new_with_slots(
        source: impl IntoIterator<Item = (Slot, &'a Arc<AccountStorageEntry>)>,
        // A slot used as a lower bound, but potentially smaller than the smallest slot in the given 'source' iterator
        min_slot: Option<Slot>,
        // highest valid slot. Only matters if source array does not contain a slot >= max_slot_inclusive.
        // An example is a slot that has accounts in the write cache at slots <= 'max_slot_inclusive' but no storages at those slots.
        // None => self.range.end = source.1.max() + 1
        // Some(slot) => self.range.end = std::cmp::max(slot, source.1.max())
        max_slot_inclusive: Option<Slot>,
    ) -> Self {
        let (storages, time) = measure!({
            let slots = match (min_slot, max_slot_inclusive) {
                (Some(min), Some(max)) => min..=max,
                (Some(min), None) => min..=Slot::MAX,
                (None, Some(max)) => Slot::MIN..=max,
                (None, None) => Slot::MIN..=Slot::MAX,
            };
            let source = source.into_iter().filter(|(slot, _)| slots.contains(slot));
            let mut storages = Box::from_iter(source);
            storages.sort_unstable_by_key(|(slot, _)| *slot);
            storages
        });
        error!("bprumo DEBUG: making sorted storages took: {time}");

        // ensure all slots are unique
        // (since all storages are sorted by slot, we only need to check our neighbor to ensure uniqueness)
        let neighbors_are_unique = |neighbors: &[(Slot, _)]| -> bool {
            debug_assert_eq!(neighbors.len(), 2);
            let a = neighbors[0];
            let b = neighbors[1];
            a.0 != b.0
        };
        assert!(
            storages.windows(2).all(neighbors_are_unique),
            "slots are not unique",
        );

        let slots = if storages.is_empty() {
            Range::default()
        } else {
            // SAFETY: We've ensured `storages` is *not* empty, so `first`/`last` will always be `Some`
            // bprumo TODO: should the input params min/max_slot ALSO change the slots range here?
            let min_slot = storages.first().unwrap().0;
            let max_slot = storages.last().unwrap().0;
            min_slot..max_slot + 1
        };

        Self { slots, storages }
    }

    /// primary method of retrieving [`(Slot, Arc<AccountStorageEntry>)`]
    pub fn iter_range(&'a self, slots: &impl RangeBounds<Slot>) -> SortedStoragesIter<'a> {
        SortedStoragesIter::new(self, slots)
    }

    pub fn range_width(&self) -> Slot {
        self.slots.end - self.slots.start
    }

    pub fn range(&self) -> &Range<Slot> {
        &self.slots
    }

    pub fn max_slot_inclusive(&self) -> Slot {
        self.slots.end.saturating_sub(1)
    }

    pub fn slot_count(&self) -> usize {
        self.storages.len()
    }

    pub fn storage_count(&self) -> usize {
        self.storages.len()
    }

    // assumption:
    // source.slot() is unique from all other items in 'source'
    /*
     * pub fn new(source: &'a [Arc<AccountStorageEntry>]) -> Self {
     *     let slots = source.iter().map(|storage| {
     *         storage.slot() // this must be unique. Will be enforced in new_with_slots
     *     });
     *     Self::new_with_slots(source.iter().zip(slots.into_iter()), None, None)
     * }
     */

    /*
     * /// create [`SortedStorages`] from `source` iterator.
     * /// `source` contains a [`Arc<AccountStorageEntry>`] and its associated slot
     * /// `source` does not have to be sorted in any way, but is assumed to not have duplicate slot #s
     */
    /*
     *     pub fn new_with_slots(
     *         source: impl Iterator<Item = (&'a Arc<AccountStorageEntry>, Slot)> + Clone,
     *         // A slot used as a lower bound, but potentially smaller than the smallest slot in the given 'source' iterator
     *         min_slot: Option<Slot>,
     *         // highest valid slot. Only matters if source array does not contain a slot >= max_slot_inclusive.
     *         // An example is a slot that has accounts in the write cache at slots <= 'max_slot_inclusive' but no storages at those slots.
     *         // None => self.range.end = source.1.max() + 1
     *         // Some(slot) => self.range.end = std::cmp::max(slot, source.1.max())
     *         max_slot_inclusive: Option<Slot>,
     *     ) -> Self {
     *         let mut min = Slot::MAX;
     *         let mut max = Slot::MIN;
     *         let mut adjust_min_max = |slot| {
     *             min = std::cmp::min(slot, min);
     *             max = std::cmp::max(slot + 1, max);
     *         };
     *         // none, either, or both of min/max could be specified
     *         if let Some(slot) = min_slot {
     *             adjust_min_max(slot);
     *         }
     *         if let Some(slot) = max_slot_inclusive {
     *             adjust_min_max(slot);
     *         }
     *
     *         let mut slot_count = 0;
     *         let mut time = Measure::start("get slot");
     *         let source_ = source.clone();
     *         let mut storage_count = 0;
     *         source_.for_each(|(_, slot)| {
     *             storage_count += 1;
     *             slot_count += 1;
     *             adjust_min_max(slot);
     *         });
     *         time.stop();
     *         let mut time2 = Measure::start("sort");
     *         let range;
     *         let mut storages = HashMap::default();
     *         if min > max {
     *             range = Range::default();
     *         } else {
     *             range = Range {
     *                 start: min,
     *                 end: max,
     *             };
     *             source.for_each(|(original_storages, slot)| {
     *                 assert!(
     *                     storages.insert(slot, original_storages).is_none(),
     *                     "slots are not unique"
     *                 ); // we should not encounter the same slot twice
     *             });
     *         }
     *         time2.stop();
     *         debug!("SortedStorages, times: {}, {}", time.as_us(), time2.as_us());
     *         Self {
     *             range,
     *             storages,
     *             bprumo_storages: Box::default(),
     *         }
     *     }
     */
}

/// Iterator over successive slots in 'storages' within 'range'.
/// This enforces sequential access so that random access does not have to be implemented.
/// Random access could be expensive with large sparse sets.
#[derive(Debug)]
pub struct SortedStoragesIter<'a> {
    /// the data to return per slot
    sorted_storages: &'a SortedStorages<'a>,

    /// range for the iterator to iterate over (start_inclusive..end_exclusive)
    slots: Range<Slot>,
    /// the slot to be returned the next time 'Next' is called
    next_slot: Slot,

    /// bprumo TODO: doc
    indices: Range<usize>,
    /// bprumo TODO: doc
    next_index: usize,
}

impl<'a> Iterator for SortedStoragesIter<'a> {
    type Item = (Slot, Option<&'a Arc<AccountStorageEntry>>);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.slots.contains(&self.next_slot) {
            //error!("bprumo DEBUG: SortedStoragesIter::next() done next slot not in slots! next index: {} (range: {:?}), next slot: {} (range: {:?}), ", self.next_index, self.indices, self.next_slot, self.slots);
            return None;
        }

        let mut result = (self.next_slot, None);

        if let Some(&(slot, storage)) = self.sorted_storages.storages.get(self.next_index) {
            if slot == self.next_slot {
                //error!("bprumo DEBUG: SortedStoragesIter::next() found slot {}! next index: {} (range: {:?}), next slot: {} (range: {:?}), ", slot, self.next_index, self.indices, self.next_slot, self.slots);
                result.1 = Some(storage);
                self.next_index += 1;
            } else {
                //error!("bprumo DEBUG: SortedStoragesIter::next() did not find slot {}! next index: {} (range: {:?}), next slot: {} (range: {:?}), ", slot, self.next_index, self.indices, self.next_slot, self.slots);
            }
        } else {
            //error!("bprumo DEBUG: SortedStoragesIter::next() done next index not in indices! next index: {} (range: {:?}), next slot: {} (range: {:?}), ", self.next_index, self.indices, self.next_slot, self.slots);
        }

        self.next_slot += 1;
        Some(result)
    }
}

impl<'a> SortedStoragesIter<'a> {
    pub fn new<R: RangeBounds<Slot>>(
        sorted_storages: &'a SortedStorages<'a>,
        range: &R,
    ) -> SortedStoragesIter<'a> {
        let storage_range = sorted_storages.range();
        let slot_start = match range.start_bound() {
            Bound::Unbounded => {
                storage_range.start // unbounded beginning starts with the min known slot (which is inclusive)
            }
            Bound::Included(x) => *x,
            Bound::Excluded(x) => *x + 1, // make inclusive
        };
        let slot_end = match range.end_bound() {
            Bound::Unbounded => {
                storage_range.end // unbounded end ends with the max known slot (which is exclusive)
            }
            Bound::Included(x) => *x + 1, // make exclusive
            Bound::Excluded(x) => *x,
        };
        // Note that the range can be outside the range of known storages.
        // This is because the storages may not be the only source of valid slots.
        // The write cache is another source of slots that 'storages' knows nothing about.
        // bprumo TODO: remove?
        let _slots = slot_start..slot_end;

        // bprumo TODO: need to find the index to start iterating from; can binary search
        // find index with slot >= range.begin
        //
        // maybe also find end index too? then next() will be faster
        //
        // Oh! get a *slice* into our storages to iterate over!

        let index_start = sorted_storages
            .storages
            .partition_point(|(slot, _)| slot < &slot_start);
        let index_end = sorted_storages
            .storages
            .partition_point(|(slot, _)| slot < &slot_end);
        //let slice_to_iterate = &storages.bprumo_storages[index_begin..index_end];

        //error!("bprumo DEBUG: SortedStoragesIter::new() slot start: {}, slot end: {}, index start: {}, index end: {}, storages slots: {:?}, storages len: {}, storages: {:?}", slot_start, slot_end, index_start, index_end, sorted_storages.slots, sorted_storages.storages.len(), sorted_storages);
        SortedStoragesIter {
            sorted_storages,
            slots: slot_start..slot_end,
            next_slot: slot_start,
            indices: index_start..index_end,
            next_index: index_start,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{accounts_db::AppendVecId, append_vec::AppendVec},
    };

    impl<'a> SortedStorages<'a> {
        pub fn new_debug(
            source: &[(Slot, &'a Arc<AccountStorageEntry>)],
            min: Slot,
            len: usize,
        ) -> Self {
            Self {
                slots: min..min + len as Slot,
                ..Self::new_for_tests(source)
            }
        }
        pub fn new_for_tests(storages: &[(Slot, &'a Arc<AccountStorageEntry>)]) -> Self {
            SortedStorages::new_with_slots(storages.iter().cloned(), None, None)
        }
        fn get(&self, slot: Slot) -> Option<&AccountStorageEntry> {
            self.storages
                .iter()
                .find(|storage| slot == storage.0)
                .map(|(_, storage)| storage.as_ref())
        }
    }

    #[test]
    fn test_sorted_storages_range_iter() {
        solana_logger::setup();
        let storages = SortedStorages::new(&[]);
        let check = |(slot, storages): (Slot, Option<&Arc<AccountStorageEntry>>)| {
            assert!(storages.is_none());
            slot
        };
        assert_eq!(
            (0..5).collect::<Vec<_>>(),
            storages.iter_range(&(..5)).map(check).collect::<Vec<_>>()
        );
        assert_eq!(
            (1..5).collect::<Vec<_>>(),
            storages.iter_range(&(1..5)).map(check).collect::<Vec<_>>()
        );
        assert_eq!(
            (0..0).collect::<Vec<_>>(),
            storages.iter_range(&(..)).map(check).collect::<Vec<_>>()
        );
        assert_eq!(
            (0..0).collect::<Vec<_>>(),
            storages.iter_range(&(1..)).map(check).collect::<Vec<_>>()
        );

        // only item is slot 3
        let s1 = create_sample_store(1);
        let storages = SortedStorages::new_for_tests(&[(3, &s1)]);
        let check = |(slot, storages): (Slot, Option<&Arc<AccountStorageEntry>>)| {
            assert!(
                (slot != 3) ^ storages.is_some(),
                "slot: {slot}, storages: {storages:?}"
            );
            slot
        };
        for start in 0..5 {
            for end in 0..5 {
                assert_eq!(
                    (start..end).collect::<Vec<_>>(),
                    storages
                        .iter_range(&(start..end))
                        .map(check)
                        .collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(
            (3..5).collect::<Vec<_>>(),
            storages.iter_range(&(..5)).map(check).collect::<Vec<_>>()
        );
        assert_eq!(
            (1..=3).collect::<Vec<_>>(),
            storages.iter_range(&(1..)).map(check).collect::<Vec<_>>()
        );
        assert_eq!(
            (3..=3).collect::<Vec<_>>(),
            storages.iter_range(&(..)).map(check).collect::<Vec<_>>()
        );

        // items in slots 2 and 4
        let store2 = create_sample_store(2);
        let store4 = create_sample_store(4);

        let storages = SortedStorages::new_for_tests(&[(2, &store2), (4, &store4)]);
        let check = |(slot, storage): (Slot, Option<&Arc<AccountStorageEntry>>)| {
            assert!(
                (slot != 2 && slot != 4)
                    ^ storage
                        .map(|storage| storage.append_vec_id() == (slot as AppendVecId))
                        .unwrap_or(false),
                "slot: {slot}, storage: {storage:?}"
            );
            slot
        };
        for start in 0..5 {
            for end in 0..5 {
                assert_eq!(
                    (start..end).collect::<Vec<_>>(),
                    storages
                        .iter_range(&(start..end))
                        .map(check)
                        .collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(
            (2..5).collect::<Vec<_>>(),
            storages.iter_range(&(..5)).map(check).collect::<Vec<_>>()
        );
        assert_eq!(
            (1..=4).collect::<Vec<_>>(),
            storages.iter_range(&(1..)).map(check).collect::<Vec<_>>()
        );
        assert_eq!(
            (2..=4).collect::<Vec<_>>(),
            storages.iter_range(&(..)).map(check).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_sorted_storages_new() {
        let store = create_sample_store(1);
        {
            let storages = SortedStorages::new_with_slots(
                [(44, &store), (33, &store)].iter().cloned(),
                Some(13),
                Some(66),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.slots, 33..45);
        }
        {
            let storages = SortedStorages::new_with_slots(
                [(44, &store), (33, &store)].iter().cloned(),
                Some(37),
                Some(66),
            );
            assert_eq!(storages.storages.len(), 1);
            assert_eq!(storages.slots, 44..45);
        }
        {
            let storages = SortedStorages::new_with_slots(
                [(44, &store), (33, &store)].iter().cloned(),
                Some(13),
                Some(41),
            );
            assert_eq!(storages.storages.len(), 1);
            assert_eq!(storages.slots, 33..34);
        }
        {
            let storages = SortedStorages::new_with_slots(
                [(44, &store), (33, &store)].iter().cloned(),
                Some(37),
                Some(41),
            );
            assert_eq!(storages.storages.len(), 0);
            assert_eq!(storages.slots, Range::default());
        }
    }

    #[test]
    #[should_panic(expected = "slots are not unique")]
    fn test_sorted_storages_duplicate_slots() {
        let store = create_sample_store(1);
        SortedStorages::new_for_tests(&[(0, &store), (0, &store)]);
    }

    #[test]
    fn test_sorted_storages_none() {
        let result = SortedStorages::new(&[]);
        assert_eq!(result.slots, Range::default());
        assert_eq!(result.slot_count(), 0);
        assert_eq!(result.storages.len(), 0);
        assert!(result.get(0).is_none());
    }

    #[test]
    fn test_sorted_storages_1() {
        let store = create_sample_store(1);
        let slot = 4;
        let result = SortedStorages::new_for_tests(&[(slot, &store)]);
        assert_eq!(result.slots, slot..slot + 1);
        assert_eq!(result.slot_count(), 1);
        assert_eq!(result.storages.len(), 1);
        assert_eq!(
            result.get(slot).unwrap().append_vec_id(),
            store.append_vec_id()
        );
    }

    fn create_sample_store(id: AppendVecId) -> Arc<AccountStorageEntry> {
        let tf = crate::append_vec::test_utils::get_append_vec_path("create_sample_store");
        let (_temp_dirs, paths) = crate::accounts_db::get_temp_accounts_paths(1).unwrap();
        let size: usize = 123;
        let slot = 0;
        let mut data = AccountStorageEntry::new(&paths[0], slot, id, size as u64);
        let av = AppendVec::new(&tf.path, true, 1024 * 1024);
        data.accounts = av;

        Arc::new(data)
    }

    #[test]
    fn test_sorted_storages_2() {
        let store = create_sample_store(1);
        let store2 = create_sample_store(2);
        let result = SortedStorages::new_for_tests(&[(4, &store), (7, &store2)]);
        assert_eq!(result.slots, 4..8);
        assert_eq!(result.slot_count(), 2);
        assert_eq!(result.storages.len(), 2);
        assert!(result.get(0).is_none());
        assert!(result.get(3).is_none());
        assert!(result.get(5).is_none());
        assert!(result.get(6).is_none());
        assert!(result.get(8).is_none());
        assert_eq!(
            result.get(4).unwrap().append_vec_id(),
            store.append_vec_id()
        );
        assert_eq!(
            result.get(7).unwrap().append_vec_id(),
            store2.append_vec_id()
        );
    }
}
