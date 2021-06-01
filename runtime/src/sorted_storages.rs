use crate::accounts_db::SnapshotStorage;
use log::*;
use solana_measure::measure::Measure;
use solana_sdk::clock::Slot;
use std::ops::Range;

pub struct SortedStorages<'a> {
    range: Range<Slot>,
    storages: Vec<Option<&'a SnapshotStorage>>,
    count: usize,
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

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn new(source: &'a [SnapshotStorage]) -> Self {
        let mut min = Slot::MAX;
        let mut max = Slot::MIN;
        let mut count = 0;
        let mut time = Measure::start("get slot");
        let slots = source
            .iter()
            .map(|storages| {
                count += storages.len();
                if !storages.is_empty() {
                    storages.first().map(|storage| {
                        let slot = storage.slot();
                        min = std::cmp::min(slot, min);
                        max = std::cmp::max(slot + 1, max);
                        slot
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
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
                    if let Some(slot) = slot {
                        let index = (slot - min) as usize;
                        assert!(storages[index].is_none());
                        storages[index] = Some(original_storages);
                    }
                });
        }
        time2.stop();
        debug!("SortedStorages, times: {}, {}", time.as_us(), time2.as_us());
        Self {
            range,
            storages,
            count,
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
            let count = source.len();
            for (storage, slot) in source {
                storages[*slot as usize] = Some(*storage);
            }

            Self {
                range,
                storages,
                count,
            }
        }
    }
}
