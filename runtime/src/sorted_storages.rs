use crate::accounts_db::SnapshotStorage;
use log::*;
use solana_measure::measure::Measure;
use solana_sdk::clock::Slot;
use std::ops::Range;

pub struct SortedStorages<'a> {
    range: Range<Slot>,
    storages: Vec<Option<&'a SnapshotStorage>>,
    slot_count: usize,
}

impl<'a> SortedStorages<'a> {
    pub fn get(&self, slot: Slot) -> Option<&SnapshotStorage> {
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

    pub fn slot_count(&self) -> usize {
        self.slot_count
    }

    // Currently only called when a testing CLI option is enabled.
    // assumptions:
    // 1. each SnapshotStorage.!is_empty()
    // 2. SnapshotStorage.first().unwrap().get_slot() is unique from all other SnapshotStorage items.
    pub fn new(source: &'a [SnapshotStorage]) -> Self {
        let slots = source
            .iter()
            .map(|storages| {
                let first = storages.first();
                assert!(first.is_some(), "SnapshotStorage.is_empty()");
                let storage = first.unwrap();
                storage.slot() // this must be unique. Will be enforced in new_with_slots
            })
            .collect::<Vec<_>>();
        Self::new_with_slots(source, &slots)
    }

    // source[i] is in slot slots[i]
    // assumptions:
    // 1. slots contains unique items
    // 2. slots and source are the same len
    pub fn new_with_slots(source: &'a [SnapshotStorage], slots: &[Slot]) -> Self {
        assert_eq!(source.len(), slots.len());
        let mut min = Slot::MAX;
        let mut max = Slot::MIN;
        let slot_count = source.len();
        let mut time = Measure::start("get slot");
        slots.iter().for_each(|slot| {
            let slot = *slot;
            min = std::cmp::min(slot, min);
            max = std::cmp::max(slot + 1, max);
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
            source
                .iter()
                .zip(slots)
                .for_each(|(original_storages, slot)| {
                    let index = (slot - min) as usize;
                    assert!(storages[index].is_none()); // we should not encounter the same slot twice
                    storages[index] = Some(original_storages);
                });
        }
        time2.stop();
        debug!("SortedStorages, times: {}, {}", time.as_us(), time2.as_us());
        Self {
            range,
            storages,
            slot_count,
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
            }
        }
    }
}
