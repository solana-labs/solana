use {
    crate::accounts_db::AccountStorageEntry,
    log::*,
    solana_measure::measure::Measure,
    solana_sdk::clock::Slot,
    std::{
        collections::HashMap,
        ops::{Bound, Range, RangeBounds},
        sync::Arc,
    },
};

/// Provide access to SnapshotStorageOnes by slot
pub struct SortedStorages<'a> {
    /// range of slots where storages exist (likely sparse)
    range: Range<Slot>,
    /// the actual storages
    /// A HashMap allows sparse storage and fast lookup of Slot -> Storage.
    /// We expect ~432k slots.
    storages: HashMap<Slot, &'a Arc<AccountStorageEntry>>,
}

impl<'a> SortedStorages<'a> {
    /// containing nothing
    pub fn empty() -> Self {
        SortedStorages {
            range: Range::default(),
            storages: HashMap::default(),
        }
    }

    /// primary method of retrieving [`(Slot, Arc<AccountStorageEntry>)`]
    pub fn iter_range<R>(&'a self, range: &R) -> SortedStoragesIter<'a>
    where
        R: RangeBounds<Slot>,
    {
        SortedStoragesIter::new(self, range)
    }

    fn get(&self, slot: Slot) -> Option<&Arc<AccountStorageEntry>> {
        self.storages.get(&slot).copied()
    }

    pub fn range_width(&self) -> Slot {
        self.range.end - self.range.start
    }

    pub fn range(&self) -> &Range<Slot> {
        &self.range
    }

    pub fn max_slot_inclusive(&self) -> Slot {
        self.range.end.saturating_sub(1)
    }

    pub fn slot_count(&self) -> usize {
        self.storages.len()
    }

    pub fn storage_count(&self) -> usize {
        self.storages.len()
    }

    // assumption:
    // source.slot() is unique from all other items in 'source'
    pub fn new(source: &'a [Arc<AccountStorageEntry>]) -> Self {
        let slots = source.iter().map(|storage| {
            storage.slot() // this must be unique. Will be enforced in new_with_slots
        });
        Self::new_with_slots(source.iter().zip(slots), None, None)
    }

    /// create [`SortedStorages`] from `source` iterator.
    /// `source` contains a [`Arc<AccountStorageEntry>`] and its associated slot
    /// `source` does not have to be sorted in any way, but is assumed to not have duplicate slot #s
    pub fn new_with_slots(
        source: impl Iterator<Item = (&'a Arc<AccountStorageEntry>, Slot)> + Clone,
        // A slot used as a lower bound, but potentially smaller than the smallest slot in the given 'source' iterator
        min_slot: Option<Slot>,
        // highest valid slot. Only matters if source array does not contain a slot >= max_slot_inclusive.
        // An example is a slot that has accounts in the write cache at slots <= 'max_slot_inclusive' but no storages at those slots.
        // None => self.range.end = source.1.max() + 1
        // Some(slot) => self.range.end = std::cmp::max(slot, source.1.max())
        max_slot_inclusive: Option<Slot>,
    ) -> Self {
        let mut min = Slot::MAX;
        let mut max = Slot::MIN;
        let mut adjust_min_max = |slot| {
            min = std::cmp::min(slot, min);
            max = std::cmp::max(slot + 1, max);
        };
        // none, either, or both of min/max could be specified
        if let Some(slot) = min_slot {
            adjust_min_max(slot);
        }
        if let Some(slot) = max_slot_inclusive {
            adjust_min_max(slot);
        }

        let mut slot_count = 0;
        let mut time = Measure::start("get slot");
        let source_ = source.clone();
        let mut storage_count = 0;
        source_.for_each(|(_, slot)| {
            storage_count += 1;
            slot_count += 1;
            adjust_min_max(slot);
        });
        time.stop();
        let mut time2 = Measure::start("sort");
        let range;
        let mut storages = HashMap::default();
        if min > max {
            range = Range::default();
        } else {
            range = Range {
                start: min,
                end: max,
            };
            source.for_each(|(original_storages, slot)| {
                assert!(
                    storages.insert(slot, original_storages).is_none(),
                    "slots are not unique"
                ); // we should not encounter the same slot twice
            });
        }
        time2.stop();
        debug!("SortedStorages, times: {}, {}", time.as_us(), time2.as_us());
        Self { range, storages }
    }
}

/// Iterator over successive slots in 'storages' within 'range'.
/// This enforces sequential access so that random access does not have to be implemented.
/// Random access could be expensive with large sparse sets.
pub struct SortedStoragesIter<'a> {
    /// range for the iterator to iterate over (start_inclusive..end_exclusive)
    range: Range<Slot>,
    /// the data to return per slot
    storages: &'a SortedStorages<'a>,
    /// the slot to be returned the next time 'Next' is called
    next_slot: Slot,
}

impl<'a> Iterator for SortedStoragesIter<'a> {
    type Item = (Slot, Option<&'a Arc<AccountStorageEntry>>);

    fn next(&mut self) -> Option<Self::Item> {
        let slot = self.next_slot;
        if slot < self.range.end {
            // iterator is still in range. Storage may or may not exist at this slot, but the iterator still needs to return the slot
            self.next_slot += 1;
            Some((slot, self.storages.get(slot)))
        } else {
            // iterator passed the end of the range, so from now on it returns None
            None
        }
    }
}

impl<'a> SortedStoragesIter<'a> {
    pub fn new<R: RangeBounds<Slot>>(
        storages: &'a SortedStorages<'a>,
        range: &R,
    ) -> SortedStoragesIter<'a> {
        let storage_range = storages.range();
        let next_slot = match range.start_bound() {
            Bound::Unbounded => {
                storage_range.start // unbounded beginning starts with the min known slot (which is inclusive)
            }
            Bound::Included(x) => *x,
            Bound::Excluded(x) => *x + 1, // make inclusive
        };
        let end_exclusive_slot = match range.end_bound() {
            Bound::Unbounded => {
                storage_range.end // unbounded end ends with the max known slot (which is exclusive)
            }
            Bound::Included(x) => *x + 1, // make exclusive
            Bound::Excluded(x) => *x,
        };
        // Note that the range can be outside the range of known storages.
        // This is because the storages may not be the only source of valid slots.
        // The write cache is another source of slots that 'storages' knows nothing about.
        let range = next_slot..end_exclusive_slot;
        SortedStoragesIter {
            range,
            storages,
            next_slot,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            accounts_db::{AccountStorageEntry, AppendVecId},
            accounts_file::AccountsFile,
            append_vec::AppendVec,
        },
        std::sync::Arc,
    };

    impl<'a> SortedStorages<'a> {
        pub fn new_debug(
            source: &[(&'a Arc<AccountStorageEntry>, Slot)],
            min: Slot,
            len: usize,
        ) -> Self {
            let mut storages = HashMap::default();
            let range = Range {
                start: min,
                end: min + len as Slot,
            };
            for (storage, slot) in source {
                storages.insert(*slot, *storage);
            }

            Self { range, storages }
        }

        pub fn new_for_tests(storages: &[&'a Arc<AccountStorageEntry>], slots: &[Slot]) -> Self {
            assert_eq!(storages.len(), slots.len());
            SortedStorages::new_with_slots(
                storages.iter().cloned().zip(slots.iter().cloned()),
                None,
                None,
            )
        }
    }

    #[test]
    fn test_sorted_storages_range_iter() {
        let storages = SortedStorages::empty();
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
        let storages = SortedStorages::new_for_tests(&[&s1], &[3]);
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

        let storages = SortedStorages::new_for_tests(&[&store2, &store4], &[2, 4]);
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
    fn test_sorted_storages_new_with_slots() {
        let store = create_sample_store(1);
        let start = 33;
        let end = 44;

        // ┌───────────────────────────────────────┐
        // │      ■        storages          ■     │
        // └──────┃──────────────────────────┃─────┘
        //     min┣ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┃max
        //        ■                          ■
        {
            let min = start + 1;
            let max = end - 1;
            let storages = SortedStorages::new_with_slots(
                [(&store, end), (&store, start)].iter().cloned(),
                Some(min),
                Some(max),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.range, start..end + 1);
        }

        // ┌───────────────────────────────────────┐
        // │               storages       ■        │    ■
        // └──────────────────────────────┃────────┘    ┃
        //                             min┣ ─ ─ ─ ─ ─ ─ ┫max
        //                                ■             ■
        {
            let min = start + 1;
            let max = end + 1;
            let storages = SortedStorages::new_with_slots(
                [(&store, end), (&store, start)].iter().cloned(),
                Some(min),
                Some(max),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.range, start..max + 1);
        }

        //        ┌───────────────────────────────────────┐
        //    ■   │     ■         storages                │
        //    ┃   └─────┃─────────────────────────────────┘
        // min┣ ─ ─ ─ ─ ┫max
        //    ■         ■
        {
            let min = start - 1;
            let max = end - 1;
            let storages = SortedStorages::new_with_slots(
                [(&store, end), (&store, start)].iter().cloned(),
                Some(min),
                Some(max),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.range, min..end + 1);
        }

        //        ┌───────────────────────────────────────┐
        //    ■   │               storages                │   ■
        //    ┃   └───────────────────────────────────────┘   ┃
        // min┣ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┫max
        //    ■                                               ■
        {
            let min = start - 1;
            let max = end + 1;
            let storages = SortedStorages::new_with_slots(
                [(&store, end), (&store, start)].iter().cloned(),
                Some(min),
                Some(max),
            );
            assert_eq!(storages.storages.len(), 2);
            assert_eq!(storages.range, min..max + 1);
        }
    }

    #[test]
    #[should_panic(expected = "slots are not unique")]
    fn test_sorted_storages_duplicate_slots() {
        let store = create_sample_store(1);
        SortedStorages::new_for_tests(&[&store, &store], &[0, 0]);
    }

    #[test]
    fn test_sorted_storages_none() {
        let result = SortedStorages::empty();
        assert_eq!(result.range, Range::default());
        assert_eq!(result.slot_count(), 0);
        assert_eq!(result.storages.len(), 0);
        assert!(result.get(0).is_none());
    }

    #[test]
    fn test_sorted_storages_1() {
        let store = create_sample_store(1);
        let slot = 4;
        let vecs = [&store];
        let result = SortedStorages::new_for_tests(&vecs, &[slot]);
        assert_eq!(
            result.range,
            Range {
                start: slot,
                end: slot + 1
            }
        );
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
        let av = AccountsFile::AppendVec(AppendVec::new(&tf.path, true, 1024 * 1024));
        data.accounts = av;

        Arc::new(data)
    }

    #[test]
    fn test_sorted_storages_2() {
        let store = create_sample_store(1);
        let store2 = create_sample_store(2);
        let slots = [4, 7];
        let vecs = [&store, &store2];
        let result = SortedStorages::new_for_tests(&vecs, &slots);
        assert_eq!(
            result.range,
            Range {
                start: slots[0],
                end: slots[1] + 1,
            }
        );
        assert_eq!(result.slot_count(), 2);
        assert_eq!(result.storages.len(), 2);
        assert!(result.get(0).is_none());
        assert!(result.get(3).is_none());
        assert!(result.get(5).is_none());
        assert!(result.get(6).is_none());
        assert!(result.get(8).is_none());
        assert_eq!(
            result.get(slots[0]).unwrap().append_vec_id(),
            store.append_vec_id()
        );
        assert_eq!(
            result.get(slots[1]).unwrap().append_vec_id(),
            store2.append_vec_id()
        );
    }
}
