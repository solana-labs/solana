use solana_sdk::clock::Slot;
use std::collections::{HashMap, HashSet};

pub type AncestorsForSerialization = HashMap<Slot, usize>;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, AbiExample)]
pub struct Ancestors {
    min: Slot,
    slots: Vec<Option<usize>>,
    count: usize,
    max: Slot,
    large_range_slots: HashSet<Slot>,
}

// some tests produce ancestors ranges that are too large such
// that we prefer to implement them in a sparse HashMap
const ANCESTORS_HASH_MAP_SIZE: u64 = 10_000;

impl From<Vec<Slot>> for Ancestors {
    fn from(source: Vec<Slot>) -> Ancestors {
        let mut result = Ancestors::default();
        if !source.is_empty() {
            result.min = Slot::MAX;
            result.max = Slot::MIN;
            source.iter().for_each(|slot| {
                result.min = std::cmp::min(result.min, *slot);
                result.max = std::cmp::max(result.max, *slot + 1);
            });
            let range = result.range();
            if range > ANCESTORS_HASH_MAP_SIZE {
                result.large_range_slots = source.into_iter().collect();
                result.min = 0;
                result.max = 0;
            } else {
                result.slots = vec![None; range as usize];
                source.into_iter().for_each(|slot| {
                    let slot = result.slot_index(&slot);
                    if result.slots[slot].is_none() {
                        result.count += 1;
                    }
                    result.slots[slot] = Some(0);
                });
            }
        }

        result
    }
}

impl From<&HashMap<Slot, usize>> for Ancestors {
    fn from(source: &HashMap<Slot, usize>) -> Ancestors {
        let mut result = Ancestors::default();
        if !source.is_empty() {
            result.min = Slot::MAX;
            result.max = Slot::MIN;
            source.iter().for_each(|(slot, _)| {
                result.min = std::cmp::min(result.min, *slot);
                result.max = std::cmp::max(result.max, *slot + 1);
            });
            let range = result.range();
            if range > ANCESTORS_HASH_MAP_SIZE {
                result.large_range_slots = source.iter().map(|(slot, _size)| *slot).collect();
                result.min = 0;
                result.max = 0;
            } else {
                result.slots = vec![None; range as usize];
                source.iter().for_each(|(slot, size)| {
                    let slot = result.slot_index(&slot);
                    if result.slots[slot].is_none() {
                        result.count += 1;
                    }
                    result.slots[slot] = Some(*size);
                });
            }
        }

        result
    }
}

impl From<&Ancestors> for HashMap<Slot, usize> {
    fn from(source: &Ancestors) -> HashMap<Slot, usize> {
        let mut result = HashMap::with_capacity(source.len());
        source.keys().iter().for_each(|slot| {
            result.insert(*slot, 0);
        });
        result
    }
}

impl Ancestors {
    pub fn keys(&self) -> Vec<Slot> {
        if self.large_range_slots.is_empty() {
            self.slots
                .iter()
                .enumerate()
                .filter_map(|(size, i)| i.map(|_| size as u64 + self.min))
                .collect::<Vec<_>>()
        } else {
            self.large_range_slots.iter().copied().collect::<Vec<_>>()
        }
    }

    pub fn get(&self, slot: &Slot) -> bool {
        if self.large_range_slots.is_empty() {
            if slot < &self.min || slot >= &self.max {
                return false;
            }
            let slot = self.slot_index(slot);
            self.slots[slot].is_some()
        } else {
            self.large_range_slots.get(slot).is_some()
        }
    }

    pub fn remove(&mut self, slot: &Slot) {
        if self.large_range_slots.is_empty() {
            if slot < &self.min || slot >= &self.max {
                return;
            }
            let slot = self.slot_index(slot);
            if self.slots[slot].is_some() {
                self.count -= 1;
                self.slots[slot] = None;
            }
        } else {
            self.large_range_slots.remove(slot);
        }
    }

    pub fn contains_key(&self, slot: &Slot) -> bool {
        if self.large_range_slots.is_empty() {
            if slot < &self.min || slot >= &self.max {
                return false;
            }
            let slot = self.slot_index(slot);
            self.slots[slot].is_some()
        } else {
            self.large_range_slots.contains(slot)
        }
    }

    pub fn len(&self) -> usize {
        if self.large_range_slots.is_empty() {
            self.count
        } else {
            self.large_range_slots.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn slot_index(&self, slot: &Slot) -> usize {
        (slot - self.min) as usize
    }

    fn range(&self) -> Slot {
        self.max - self.min
    }
}
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::contains::Contains;
    use log::*;
    use solana_measure::measure::Measure;
    use std::collections::HashSet;

    impl std::iter::FromIterator<(Slot, usize)> for Ancestors {
        fn from_iter<I>(iter: I) -> Self
        where
            I: IntoIterator<Item = (Slot, usize)>,
        {
            let mut data = Vec::new();
            for i in iter {
                data.push(i);
            }
            Ancestors::from(data)
        }
    }

    impl From<Vec<(Slot, usize)>> for Ancestors {
        fn from(source: Vec<(Slot, usize)>) -> Ancestors {
            Ancestors::from(source.into_iter().map(|(slot, _)| slot).collect::<Vec<_>>())
        }
    }
    impl Ancestors {
        pub fn insert(&mut self, mut slot: Slot, size: usize) {
            if self.large_range_slots.is_empty() {
                if slot < self.min || slot >= self.max {
                    let new_min = std::cmp::min(self.min, slot);
                    let new_max = std::cmp::max(self.max, slot + 1);
                    let new_range = new_max - new_min;
                    if new_min == self.min {
                        self.max = slot + 1;
                        self.slots.resize(new_range as usize, None);
                    } else {
                        // min changed
                        let mut new_slots = vec![None; new_range as usize];
                        self.slots.iter().enumerate().for_each(|(i, size)| {
                            new_slots[i as usize + self.min as usize - slot as usize] = *size
                        });
                        self.slots = new_slots;
                        self.min = slot;
                        // fall through and set this value in
                    }
                }
                slot -= self.min;
                if self.slots[slot as usize].is_none() {
                    self.count += 1;
                }
                self.slots[slot as usize] = Some(size);
            } else {
                self.large_range_slots.insert(slot);
            }
        }
    }

    #[test]
    fn test_ancestors_permutations() {
        solana_logger::setup();
        let mut ancestors = Ancestors::default();
        let mut hash = HashMap::new();

        let min = 101_000;
        let width = 400_000;
        let dead = 19;

        let mut slot = min;
        while hash.len() < width {
            slot += 1;
            if slot % dead == 0 {
                continue;
            }
            hash.insert(slot, 0);
            ancestors.insert(slot, 0);
        }
        compare_ancestors(&hash, &ancestors);

        let max = slot + 1;

        let mut time = Measure::start("");
        let mut count = 0;
        for slot in (min - 10)..max + 100 {
            if hash.contains(&slot) {
                count += 1;
            }
        }
        time.stop();

        let mut time2 = Measure::start("");
        let mut count2 = 0;
        for slot in (min - 10)..max + 100 {
            if ancestors.contains_key(&slot) {
                count2 += 1;
            }
        }
        time2.stop();
        info!(
            "{}ms, {}ms, {} ratio",
            time.as_ms(),
            time2.as_ms(),
            time.as_ns() / time2.as_ns()
        );
        assert_eq!(count, count2);
    }

    fn compare_ancestors(hashset: &HashMap<u64, usize>, ancestors: &Ancestors) {
        assert_eq!(hashset.len(), ancestors.len());
        assert_eq!(hashset.is_empty(), ancestors.is_empty());
        let mut min = u64::MAX;
        let mut max = 0;
        for item in hashset.iter() {
            let key = item.0;
            min = std::cmp::min(min, *key);
            max = std::cmp::max(max, *key);
            assert!(ancestors.get(&key));
        }
        for slot in min - 1..max + 2 {
            assert_eq!(ancestors.get(&slot), hashset.contains(&slot));
        }
    }

    #[test]
    fn test_ancestors_smaller() {
        solana_logger::setup();

        for width in 0..34 {
            let mut hash = HashSet::new();

            let min = 1_010_000;
            let dead = 19;

            let mut slot = min;
            let mut slots = Vec::new();
            while hash.len() < width {
                slot += 1;
                if slot % dead == 0 {
                    continue;
                }
                hash.insert(slot);
                slots.push((slot, 0));
            }
            let ancestors = Ancestors::from(slots);

            let max = slot + 1;
            let passes = 1;
            let mut time = Measure::start("");
            let mut count = 0;
            for _pass in 0..passes {
                for slot in (min - 10)..max + 100 {
                    if hash.contains(&slot) {
                        count += 1;
                    }
                }
            }
            time.stop();

            let mut time2 = Measure::start("");
            let mut count2 = 0;
            for _pass in 0..passes {
                for slot in (min - 10)..max + 100 {
                    if ancestors.contains_key(&slot) {
                        count2 += 1;
                    }
                }
            }
            time2.stop();
            info!(
                "{}, {}, {}",
                time.as_ms(),
                time2.as_ms(),
                time.as_ns() / time2.as_ns()
            );
            assert_eq!(count, count2);
        }
    }
}
