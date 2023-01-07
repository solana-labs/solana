use {
    crate::accounts_db::SnapshotStorage,
    log::*,
    solana_measure::measure::Measure,
    solana_sdk::clock::Slot,
    std::ops::{Bound, Range, RangeBounds},
};

/// Provide access to SnapshotStorages sorted by slot
pub struct SortedStorages<'a> {
    /// range of slots where storages exist (likely sparse)
    range: Range<Slot>,
    /// the actual storages. index is (slot - range.start)
    storages: Vec<Option<&'a SnapshotStorage>>,
    slot_count: usize,
    storage_count: usize,
}

impl<'a> SortedStorages<'a> {
    /// containing nothing
    pub fn empty() -> Self {
        SortedStorages {
            range: Range::default(),
            storages: Vec::default(),
            slot_count: 0,
            storage_count: 0,
        }
    }

    /// primary method of retrieving (Slot, SnapshotStorage)
    pub fn iter_range<R>(&'a self, range: &R) -> SortedStoragesIter<'a>
    where
        R: RangeBounds<Slot>,
    {
        SortedStoragesIter::new(self, range)
    }

    fn get(&self, slot: Slot) -> Option<&SnapshotStorage> {
        if !self.range.contains(&slot) {
            None
        } else {
            let index = (slot - self.range.start) as usize;
            self.storages[index]
        }
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
        self.slot_count
    }

    pub fn storage_count(&self) -> usize {
        self.storage_count
    }

    // assumptions:
    // 1. each SnapshotStorage.!is_empty()
    // 2. SnapshotStorage.first().unwrap().get_slot() is unique from all other SnapshotStorage items.
    pub fn new(source: &'a [SnapshotStorage]) -> Self {
        let slots = source.iter().map(|storages| {
            let first = storages.first();
            assert!(first.is_some(), "SnapshotStorage.is_empty()");
            let storage = first.unwrap();
            storage.slot() // this must be unique. Will be enforced in new_with_slots
        });
        Self::new_with_slots(source.iter().zip(slots.into_iter()), None, None)
    }

    /// create `SortedStorages` from 'source' iterator.
    /// 'source' contains a SnapshotStorage and its associated slot
    /// 'source' does not have to be sorted in any way, but is assumed to not have duplicate slot #s
    pub fn new_with_slots(
        source: impl Iterator<Item = (&'a SnapshotStorage, Slot)> + Clone,
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
        source_.for_each(|(storages, slot)| {
            storage_count += storages.len();
            slot_count += 1;
            adjust_min_max(slot);
        });
        time.stop();
        let mut time2 = Measure::start("sort");
        let range;
        let mut storages;
        if min > max {
            range = Range::default();
            storages = vec![];
        } else {
            range = Range {
                start: min,
                end: max,
            };
            let len = max - min;
            storages = vec![None; len as usize];
            source.for_each(|(original_storages, slot)| {
                let index = (slot - min) as usize;
                assert!(storages[index].is_none(), "slots are not unique"); // we should not encounter the same slot twice
                storages[index] = Some(original_storages);
            });
        }
        time2.stop();
        debug!("SortedStorages, times: {}, {}", time.as_us(), time2.as_us());
        Self {
            range,
            storages,
            slot_count,
            storage_count,
        }
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
    type Item = (Slot, Option<&'a SnapshotStorage>);

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
pub mod tests {
    use super::*;
    impl<'a> SortedStorages<'a> {
        pub fn new_debug(source: &[(&'a SnapshotStorage, Slot)], min: Slot, len: usize) -> Self {
            let mut storages = vec![None; len];
            let range = Range {
                start: min,
                end: min + len as Slot,
            };
            let slot_count = source.len();
            for (storage, slot) in source {
                storages[*slot as usize] = Some(*storage);
            }

            Self {
                range,
                storages,
                slot_count,
                storage_count: 0,
            }
        }

        pub fn new_for_tests(storages: &[&'a SnapshotStorage], slots: &[Slot]) -> Self {
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
        let check = |(slot, storages): (Slot, Option<&SnapshotStorage>)| {
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
        let s1 = Vec::new();
        let storages = SortedStorages::new_for_tests(&[&s1], &[3]);
        let check = |(slot, storages): (Slot, Option<&SnapshotStorage>)| {
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
        let s2 = Vec::with_capacity(2);
        let s4 = Vec::with_capacity(4);
        let storages = SortedStorages::new_for_tests(&[&s2, &s4], &[2, 4]);
        let check = |(slot, storages): (Slot, Option<&SnapshotStorage>)| {
            assert!(
                (slot != 2 && slot != 4)
                    ^ storages
                        .map(|storages| storages.capacity() == (slot as usize))
                        .unwrap_or(false),
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
    #[should_panic(expected = "SnapshotStorage.is_empty()")]
    fn test_sorted_storages_empty() {
        SortedStorages::new(&[Vec::new()]);
    }

    #[test]
    #[should_panic(expected = "slots are not unique")]
    fn test_sorted_storages_duplicate_slots() {
        SortedStorages::new_for_tests(&[&Vec::new(), &Vec::new()], &[0, 0]);
    }

    #[test]
    fn test_sorted_storages_none() {
        let result = SortedStorages::empty();
        assert_eq!(result.range, Range::default());
        assert_eq!(result.slot_count, 0);
        assert_eq!(result.storages.len(), 0);
        assert!(result.get(0).is_none());
    }

    #[test]
    fn test_sorted_storages_1() {
        let vec = vec![];
        let vec_check = vec.clone();
        let slot = 4;
        let vecs = [&vec];
        let result = SortedStorages::new_for_tests(&vecs, &[slot]);
        assert_eq!(
            result.range,
            Range {
                start: slot,
                end: slot + 1
            }
        );
        assert_eq!(result.slot_count, 1);
        assert_eq!(result.storages.len(), 1);
        assert_eq!(result.get(slot).unwrap().len(), vec_check.len());
    }

    #[test]
    fn test_sorted_storages_2() {
        let vec = vec![];
        let vec_check = vec.clone();
        let slots = [4, 7];
        let vecs = [&vec, &vec];
        let result = SortedStorages::new_for_tests(&vecs, &slots);
        assert_eq!(
            result.range,
            Range {
                start: slots[0],
                end: slots[1] + 1,
            }
        );
        assert_eq!(result.slot_count, 2);
        assert_eq!(result.storages.len() as Slot, slots[1] - slots[0] + 1);
        assert!(result.get(0).is_none());
        assert!(result.get(3).is_none());
        assert!(result.get(5).is_none());
        assert!(result.get(6).is_none());
        assert!(result.get(8).is_none());
        assert_eq!(result.get(slots[0]).unwrap().len(), vec_check.len());
        assert_eq!(result.get(slots[1]).unwrap().len(), vec_check.len());
    }
}
