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

/// Provide access to snapshot storages by slot, sorted and contiguous
#[derive(Debug)]
pub struct SortedStorages<'a> {
    /// range of slots where storages exist (likely sparse)
    slots: Range<Slot>,
    /// the actual storages, sorted by slot
    /// We expect ~432k slots.
    storages: Box<[(Slot, &'a Arc<AccountStorageEntry>)]>,
}

impl<'a> SortedStorages<'a> {
    // assumption:
    // source.slot() is unique from all other items in 'source'
    pub fn new(source: &'a [Arc<AccountStorageEntry>]) -> Self {
        let source_with_slots = source.into_iter().map(|storage| (storage.slot(), storage));
        Self::new_with_slots(source_with_slots, None, None)
    }

    /// create [`SortedStorages`] from `source` iterator.
    /// `source` contains a [`Arc<AccountStorageEntry>`] and its associated slot
    /// `source` does not have to be sorted in any way, but is assumed to not have duplicate slot #s
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
            let mut storages = Box::from_iter(source.into_iter());
            storages.sort_unstable_by_key(|(slot, _)| *slot);
            storages
        });
        debug!("sorting storages took {time}");

        // ensure all slots are unique
        // (since all storages are sorted by slot, we only need to check our neighbor to ensure uniqueness)
        let neighbors_are_unique = |neighbors: &[(Slot, _)]| -> bool {
            debug_assert_eq!(neighbors.len(), 2);
            let a = neighbors[0];
            let b = neighbors[1];
            a.0 != b.0
        };
        let (all_slots_are_unique, time) = measure!(storages.windows(2).all(neighbors_are_unique));
        assert!(all_slots_are_unique, "slots are not unique");
        debug!("ensuring all slots are unique took {time}");

        let slots = if storages.is_empty() {
            Range::default()
        } else {
            // SAFETY: We've ensured `storages` is *not* empty, so `first`/`last` will always be `Some`
            let min_slot_source = storages.first().unwrap().0;
            let min_slot_param = min_slot.unwrap_or(Slot::MAX);
            let min_slot = std::cmp::min(min_slot_source, min_slot_param);

            let max_slot_source = storages.last().unwrap().0;
            let max_slot_param = max_slot_inclusive.unwrap_or(Slot::MIN);
            let max_slot = std::cmp::max(max_slot_source, max_slot_param);

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
}

/// Iterator over successive slots in 'storages' within 'range'.
/// This enforces sequential access so that random access does not have to be implemented.
/// Random access could be expensive with large sparse sets.
#[derive(Debug)]
pub struct SortedStoragesIter<'a> {
    /// the data to return per slot
    sorted_storages: &'a SortedStorages<'a>,

    /// range of slots to iterate over
    slots: Range<Slot>,
    /// the slot to be returned the next time 'Next' is called
    next_slot: Slot,

    /// the indices of `sorted_storages` to iterate over
    indices: Range<usize>,
    /// the index in `sorted_storages` to lookup when `next()` is called
    next_index: usize,
}

impl<'a> Iterator for SortedStoragesIter<'a> {
    type Item = (Slot, Option<&'a Arc<AccountStorageEntry>>);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.slots.contains(&self.next_slot) {
            debug!("SortedStoragesIter::next() done! next slot: {}, slots: {:?}, next index: {}, indices: {:?}", self.next_slot, self.slots, self.next_index, self.indices);
            return None;
        }

        let mut result = (self.next_slot, None);

        if let Some(&(slot, storage)) = self.sorted_storages.storages.get(self.next_index) {
            if slot == self.next_slot {
                debug!(
                    "SortedStoragesIter::next() next slot {} matches at index {}",
                    self.next_slot, self.next_index
                );
                result.1 = Some(storage);
                self.next_index += 1;
            } else {
                debug!("SortedStoragesIter::next() next slot {} does not match at index {}, slot found: {}", self.next_slot, self.next_index, slot);
            }
        } else {
            debug!(
                "SortedStoragesIter::next() next index {} is not in storages, next slot: {}",
                self.next_index, self.next_slot
            );
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
        // Note that the range can be outside the range of known storages.
        // This is because the storages may not be the only source of valid slots.
        // The write cache is another source of slots that 'storages' knows nothing about.
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

        let index_start = sorted_storages
            .storages
            .partition_point(|(slot, _)| slot < &slot_start);
        let index_end = sorted_storages
            .storages
            .partition_point(|(slot, _)| slot < &slot_end);

        let new = SortedStoragesIter {
            sorted_storages,
            slots: slot_start..slot_end,
            next_slot: slot_start,
            indices: index_start..index_end,
            next_index: index_start,
        };
        debug!(
            "SortedStoragesIter::new() slots: {:?}, indices: {:?}",
            new.slots, new.indices
        );
        new
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
            assert_eq!(storages.slots, 13..67);
        }
        {
            let storages = SortedStorages::new_with_slots(
                [(44, &store), (33, &store)].iter().cloned(),
                Some(37),
                Some(66),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.slots, 33..67);
        }
        {
            let storages = SortedStorages::new_with_slots(
                [(44, &store), (33, &store)].iter().cloned(),
                Some(13),
                Some(41),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.slots, 13..45);
        }
        {
            let storages = SortedStorages::new_with_slots(
                [(44, &store), (33, &store)].iter().cloned(),
                Some(37),
                Some(41),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.slots, 33..45);
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
