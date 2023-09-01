//! functionally similar to a hashset
//! Relies on there being a sliding window of key values. The key values continue to increase.
//! Old key values are removed from the lesser values and do not accumulate.

use {bv::BitVec, solana_sdk::clock::Slot, std::collections::HashSet};

#[derive(Debug, Default, AbiExample, Clone)]
pub struct RollingBitField {
    max_width: u64,
    min: u64,
    max_exclusive: u64,
    bits: BitVec,
    count: usize,
    // These are items that are true and lower than min.
    // They would cause us to exceed max_width if we stored them in our bit field.
    // We only expect these items in conditions where there is some other bug in the system
    //  or in testing when large ranges are created.
    excess: HashSet<u64>,
}

impl PartialEq<RollingBitField> for RollingBitField {
    fn eq(&self, other: &Self) -> bool {
        // 2 instances could have different internal data for the same values,
        // so we have to compare data.
        self.len() == other.len() && {
            for item in self.get_all() {
                if !other.contains(&item) {
                    return false;
                }
            }
            true
        }
    }
}

/// functionally similar to a hashset
/// Relies on there being a sliding window of key values. The key values continue to increase.
/// Old key values are removed from the lesser values and do not accumulate.
impl RollingBitField {
    pub fn new(max_width: u64) -> Self {
        assert!(max_width > 0);
        assert!(max_width.is_power_of_two()); // power of 2 to make dividing a shift
        let bits = BitVec::new_fill(false, max_width);
        Self {
            max_width,
            bits,
            count: 0,
            min: 0,
            max_exclusive: 0,
            excess: HashSet::new(),
        }
    }

    // find the array index
    fn get_address(&self, key: &u64) -> u64 {
        key % self.max_width
    }

    pub fn range_width(&self) -> u64 {
        // note that max isn't updated on remove, so it can be above the current max
        self.max_exclusive - self.min
    }

    pub fn min(&self) -> Option<u64> {
        if self.is_empty() {
            None
        } else if self.excess.is_empty() {
            Some(self.min)
        } else {
            let mut min = if self.all_items_in_excess() {
                u64::MAX
            } else {
                self.min
            };
            for item in &self.excess {
                min = std::cmp::min(min, *item);
            }
            Some(min)
        }
    }

    pub fn insert(&mut self, key: u64) {
        let mut bits_empty = self.count == 0 || self.all_items_in_excess();
        let update_bits = if bits_empty {
            true // nothing in bits, so in range
        } else if key < self.min {
            // bits not empty and this insert is before min, so add to excess
            if self.excess.insert(key) {
                self.count += 1;
            }
            false
        } else if key < self.max_exclusive {
            true // fits current bit field range
        } else {
            // key is >= max
            let new_max = key + 1;
            loop {
                let new_width = new_max.saturating_sub(self.min);
                if new_width <= self.max_width {
                    // this key will fit the max range
                    break;
                }

                // move the min item from bits to excess and then purge from min to make room for this new max
                let inserted = self.excess.insert(self.min);
                assert!(inserted);

                let key = self.min;
                let address = self.get_address(&key);
                self.bits.set(address, false);
                self.purge(&key);

                if self.all_items_in_excess() {
                    // if we moved the last existing item to excess, then we are ready to insert the new item in the bits
                    bits_empty = true;
                    break;
                }
            }

            true // moved things to excess if necessary, so update bits with the new entry
        };

        if update_bits {
            let address = self.get_address(&key);
            let value = self.bits.get(address);
            if !value {
                self.bits.set(address, true);
                if bits_empty {
                    self.min = key;
                    self.max_exclusive = key + 1;
                } else {
                    self.min = std::cmp::min(self.min, key);
                    self.max_exclusive = std::cmp::max(self.max_exclusive, key + 1);
                    assert!(
                        self.min + self.max_width >= self.max_exclusive,
                        "min: {}, max: {}, max_width: {}",
                        self.min,
                        self.max_exclusive,
                        self.max_width
                    );
                }
                self.count += 1;
            }
        }
    }

    /// remove key from set, return if item was in the set
    pub fn remove(&mut self, key: &u64) -> bool {
        if key >= &self.min {
            // if asked to remove something bigger than max, then no-op
            if key < &self.max_exclusive {
                let address = self.get_address(key);
                let get = self.bits.get(address);
                if get {
                    self.count -= 1;
                    self.bits.set(address, false);
                    self.purge(key);
                }
                get
            } else {
                false
            }
        } else {
            // asked to remove something < min. would be in excess if it exists
            let remove = self.excess.remove(key);
            if remove {
                self.count -= 1;
            }
            remove
        }
    }

    fn all_items_in_excess(&self) -> bool {
        self.excess.len() == self.count
    }

    // after removing 'key' where 'key' = min, make min the correct new min value
    fn purge(&mut self, key: &u64) {
        if self.count > 0 && !self.all_items_in_excess() {
            if key == &self.min {
                let start = self.min + 1; // min just got removed
                for key in start..self.max_exclusive {
                    if self.contains_assume_in_range(&key) {
                        self.min = key;
                        break;
                    }
                }
            }
        } else {
            // The idea is that there are no items in the bitfield anymore.
            // But, there MAY be items in excess. The model works such that items < min go into excess.
            // So, after purging all items from bitfield, we hold max to be what it previously was, but set min to max.
            // Thus, if we lookup >= max, answer is always false without having to look in excess.
            // If we changed max here to 0, we would lose the ability to know the range of items in excess (if any).
            // So, now, with min updated = max:
            // If we lookup < max, then we first check min.
            // If >= min, then we look in bitfield.
            // Otherwise, we look in excess since the request is < min.
            // So, resetting min like this after a remove results in the correct behavior for the model.
            // Later, if we insert and there are 0 items total (excess + bitfield), then we reset min/max to reflect the new item only.
            self.min = self.max_exclusive;
        }
    }

    fn contains_assume_in_range(&self, key: &u64) -> bool {
        // the result may be aliased. Caller is responsible for determining key is in range.
        let address = self.get_address(key);
        self.bits.get(address)
    }

    // This is the 99% use case.
    // This needs be fast for the most common case of asking for key >= min.
    pub fn contains(&self, key: &u64) -> bool {
        if key < &self.max_exclusive {
            if key >= &self.min {
                // in the bitfield range
                self.contains_assume_in_range(key)
            } else {
                self.excess.contains(key)
            }
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn max_exclusive(&self) -> u64 {
        self.max_exclusive
    }

    pub fn max_inclusive(&self) -> u64 {
        self.max_exclusive.saturating_sub(1)
    }

    /// return all items < 'max_slot_exclusive'
    pub fn get_all_less_than(&self, max_slot_exclusive: Slot) -> Vec<u64> {
        let mut all = Vec::with_capacity(self.count);
        self.excess.iter().for_each(|slot| {
            if slot < &max_slot_exclusive {
                all.push(*slot)
            }
        });
        for key in self.min..self.max_exclusive {
            if key >= max_slot_exclusive {
                break;
            }

            if self.contains_assume_in_range(&key) {
                all.push(key);
            }
        }
        all
    }

    /// return highest item < 'max_slot_exclusive'
    pub fn get_prior(&self, max_slot_exclusive: Slot) -> Option<Slot> {
        let mut slot = max_slot_exclusive.saturating_sub(1);
        self.min().and_then(|min| {
            loop {
                if self.contains(&slot) {
                    return Some(slot);
                }
                slot = slot.saturating_sub(1);
                if slot == 0 || slot < min {
                    break;
                }
            }
            None
        })
    }

    pub fn get_all(&self) -> Vec<u64> {
        let mut all = Vec::with_capacity(self.count);
        self.excess.iter().for_each(|slot| all.push(*slot));
        for key in self.min..self.max_exclusive {
            if self.contains_assume_in_range(&key) {
                all.push(key);
            }
        }
        all
    }
}

#[cfg(test)]
pub mod tests {
    use {super::*, log::*, solana_measure::measure::Measure};

    impl RollingBitField {
        pub fn clear(&mut self) {
            *self = Self::new(self.max_width);
        }
    }

    #[test]
    fn test_get_all_less_than() {
        solana_logger::setup();
        let len = 16;
        let mut bitfield = RollingBitField::new(len);
        assert!(bitfield.get_all_less_than(0).is_empty());
        bitfield.insert(0);
        assert!(bitfield.get_all_less_than(0).is_empty());
        assert_eq!(bitfield.get_all_less_than(1), vec![0]);
        bitfield.insert(1);
        assert_eq!(bitfield.get_all_less_than(1), vec![0]);
        let last_item_not_in_excess = len - 1;
        bitfield.insert(last_item_not_in_excess);
        assert!(bitfield.excess.is_empty());
        assert_eq!(
            bitfield.get_all_less_than(last_item_not_in_excess),
            vec![0, 1]
        );
        assert_eq!(
            bitfield.get_all_less_than(last_item_not_in_excess + 1),
            vec![0, 1, last_item_not_in_excess]
        );
        let first_item_in_excess = last_item_not_in_excess + 1;
        bitfield.insert(first_item_in_excess);
        assert!(bitfield.excess.contains(&0));
        assert_eq!(
            bitfield.get_all_less_than(last_item_not_in_excess),
            vec![0, 1]
        );
        assert_eq!(
            bitfield.get_all_less_than(last_item_not_in_excess + 1),
            vec![0, 1, last_item_not_in_excess]
        );
        assert_eq!(
            bitfield.get_all_less_than(first_item_in_excess + 1),
            vec![0, 1, last_item_not_in_excess, first_item_in_excess]
        );

        bitfield.insert(len * 2);
        let mut less = bitfield.get_all_less_than(len * 2);
        less.sort_unstable();
        assert_eq!(
            vec![0, 1, last_item_not_in_excess, first_item_in_excess],
            less
        );
        let mut less = bitfield.get_all_less_than(len * 2 + 1);
        less.sort_unstable();
        assert_eq!(
            vec![0, 1, last_item_not_in_excess, first_item_in_excess, len * 2],
            less
        );
    }

    #[test]
    fn test_bitfield_delete_non_excess() {
        solana_logger::setup();
        let len = 16;
        let mut bitfield = RollingBitField::new(len);
        assert_eq!(bitfield.min(), None);

        bitfield.insert(0);
        assert_eq!(bitfield.min(), Some(0));
        let too_big = len + 1;
        bitfield.insert(too_big);
        assert!(bitfield.contains(&0));
        assert!(bitfield.contains(&too_big));
        assert_eq!(bitfield.len(), 2);
        assert_eq!(bitfield.excess.len(), 1);
        assert_eq!(bitfield.min, too_big);
        assert_eq!(bitfield.min(), Some(0));
        assert_eq!(bitfield.max_exclusive, too_big + 1);

        // delete the thing that is NOT in excess
        bitfield.remove(&too_big);
        assert_eq!(bitfield.min, too_big + 1);
        assert_eq!(bitfield.max_exclusive, too_big + 1);
        let too_big_times_2 = too_big * 2;
        bitfield.insert(too_big_times_2);
        assert!(bitfield.contains(&0));
        assert!(bitfield.contains(&too_big_times_2));
        assert_eq!(bitfield.len(), 2);
        assert_eq!(bitfield.excess.len(), 1);
        assert_eq!(bitfield.min(), bitfield.excess.iter().min().copied());
        assert_eq!(bitfield.min, too_big_times_2);
        assert_eq!(bitfield.max_exclusive, too_big_times_2 + 1);

        bitfield.remove(&0);
        bitfield.remove(&too_big_times_2);
        assert!(bitfield.is_empty());
        let other = 5;
        bitfield.insert(other);
        assert!(bitfield.contains(&other));
        assert!(bitfield.excess.is_empty());
        assert_eq!(bitfield.min, other);
        assert_eq!(bitfield.max_exclusive, other + 1);
    }

    #[test]
    fn test_bitfield_insert_excess() {
        solana_logger::setup();
        let len = 16;
        let mut bitfield = RollingBitField::new(len);

        bitfield.insert(0);
        let too_big = len + 1;
        bitfield.insert(too_big);
        assert!(bitfield.contains(&0));
        assert!(bitfield.contains(&too_big));
        assert_eq!(bitfield.len(), 2);
        assert_eq!(bitfield.excess.len(), 1);
        assert!(bitfield.excess.contains(&0));
        assert_eq!(bitfield.min, too_big);
        assert_eq!(bitfield.max_exclusive, too_big + 1);

        // delete the thing that IS in excess
        // this does NOT affect min/max
        bitfield.remove(&0);
        assert_eq!(bitfield.min, too_big);
        assert_eq!(bitfield.max_exclusive, too_big + 1);
        // re-add to excess
        bitfield.insert(0);
        assert!(bitfield.contains(&0));
        assert!(bitfield.contains(&too_big));
        assert_eq!(bitfield.len(), 2);
        assert_eq!(bitfield.excess.len(), 1);
        assert_eq!(bitfield.min, too_big);
        assert_eq!(bitfield.max_exclusive, too_big + 1);
    }

    #[test]
    fn test_bitfield_permutations() {
        solana_logger::setup();
        let mut bitfield = RollingBitField::new(2097152);
        let mut hash = HashSet::new();

        let min = 101_000;
        let width = 400_000;
        let dead = 19;

        let mut slot = min;
        while hash.len() < width {
            slot += 1;
            if slot % dead == 0 {
                continue;
            }
            hash.insert(slot);
            bitfield.insert(slot);
        }
        compare(&hash, &bitfield);

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
            if bitfield.contains(&slot) {
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

    #[test]
    #[should_panic(expected = "max_width.is_power_of_two()")]
    fn test_bitfield_power_2() {
        let _ = RollingBitField::new(3);
    }

    #[test]
    #[should_panic(expected = "max_width > 0")]
    fn test_bitfield_0() {
        let _ = RollingBitField::new(0);
    }

    fn setup_empty(width: u64) -> RollingBitFieldTester {
        let bitfield = RollingBitField::new(width);
        let hash_set = HashSet::new();
        RollingBitFieldTester { bitfield, hash_set }
    }

    struct RollingBitFieldTester {
        pub bitfield: RollingBitField,
        pub hash_set: HashSet<u64>,
    }

    impl RollingBitFieldTester {
        fn insert(&mut self, slot: u64) {
            self.bitfield.insert(slot);
            self.hash_set.insert(slot);
            assert!(self.bitfield.contains(&slot));
            compare(&self.hash_set, &self.bitfield);
        }
        fn remove(&mut self, slot: &u64) -> bool {
            let result = self.bitfield.remove(slot);
            assert_eq!(result, self.hash_set.remove(slot));
            assert!(!self.bitfield.contains(slot));
            self.compare();
            result
        }
        fn compare(&self) {
            compare(&self.hash_set, &self.bitfield);
        }
    }

    fn setup_wide(width: u64, start: u64) -> RollingBitFieldTester {
        let mut tester = setup_empty(width);

        tester.compare();
        tester.insert(start);
        tester.insert(start + 1);
        tester
    }

    #[test]
    fn test_bitfield_insert_wide() {
        solana_logger::setup();
        let width = 16;
        let start = 0;
        let mut tester = setup_wide(width, start);

        let slot = start + width;
        let all = tester.bitfield.get_all();
        // higher than max range by 1
        tester.insert(slot);
        let bitfield = tester.bitfield;
        for slot in all {
            assert!(bitfield.contains(&slot));
        }
        assert_eq!(bitfield.excess.len(), 1);
        assert_eq!(bitfield.count, 3);
    }

    #[test]
    fn test_bitfield_insert_wide_before() {
        solana_logger::setup();
        let width = 16;
        let start = 100;
        let mut bitfield = setup_wide(width, start).bitfield;

        let slot = start + 1 - width;
        // assert here - would make min too low, causing too wide of a range
        bitfield.insert(slot);
        assert_eq!(1, bitfield.excess.len());
        assert_eq!(3, bitfield.count);
        assert!(bitfield.contains(&slot));
    }

    #[test]
    fn test_bitfield_insert_wide_before_ok() {
        solana_logger::setup();
        let width = 16;
        let start = 100;
        let mut bitfield = setup_wide(width, start).bitfield;

        let slot = start + 2 - width; // this item would make our width exactly equal to what is allowed, but it is also inserting prior to min
        bitfield.insert(slot);
        assert_eq!(1, bitfield.excess.len());
        assert!(bitfield.contains(&slot));
        assert_eq!(3, bitfield.count);
    }

    #[test]
    fn test_bitfield_contains_wide_no_assert() {
        {
            let width = 16;
            let start = 0;
            let bitfield = setup_wide(width, start).bitfield;

            let mut slot = width;
            assert!(!bitfield.contains(&slot));
            slot += 1;
            assert!(!bitfield.contains(&slot));
        }
        {
            let width = 16;
            let start = 100;
            let bitfield = setup_wide(width, start).bitfield;

            // too large
            let mut slot = width;
            assert!(!bitfield.contains(&slot));
            slot += 1;
            assert!(!bitfield.contains(&slot));
            // too small, before min
            slot = 0;
            assert!(!bitfield.contains(&slot));
        }
    }

    #[test]
    fn test_bitfield_remove_wide() {
        let width = 16;
        let start = 0;
        let mut tester = setup_wide(width, start);
        let slot = width;
        assert!(!tester.remove(&slot));
    }

    #[test]
    fn test_bitfield_excess2() {
        solana_logger::setup();
        let width = 16;
        let mut tester = setup_empty(width);
        let slot = 100;
        // insert 1st slot
        tester.insert(slot);
        assert!(tester.bitfield.excess.is_empty());

        // insert a slot before the previous one. this is 'excess' since we don't use this pattern in normal operation
        let slot2 = slot - 1;
        tester.insert(slot2);
        assert_eq!(tester.bitfield.excess.len(), 1);

        // remove the 1st slot. we will be left with only excess
        tester.remove(&slot);
        assert!(tester.bitfield.contains(&slot2));
        assert_eq!(tester.bitfield.excess.len(), 1);

        // re-insert at valid range, making sure we don't insert into excess
        tester.insert(slot);
        assert_eq!(tester.bitfield.excess.len(), 1);

        // remove the excess slot.
        tester.remove(&slot2);
        assert!(tester.bitfield.contains(&slot));
        assert!(tester.bitfield.excess.is_empty());

        // re-insert the excess slot
        tester.insert(slot2);
        assert_eq!(tester.bitfield.excess.len(), 1);
    }

    #[test]
    fn test_bitfield_excess() {
        solana_logger::setup();
        // start at slot 0 or a separate, higher slot
        for width in [16, 4194304].iter() {
            let width = *width;
            let mut tester = setup_empty(width);
            for start in [0, width * 5].iter().cloned() {
                // recreate means create empty bitfield with each iteration, otherwise re-use
                for recreate in [false, true].iter().cloned() {
                    let max = start + 3;
                    // first root to add
                    for slot in start..max {
                        // subsequent roots to add
                        for slot2 in (slot + 1)..max {
                            // reverse_slots = 1 means add slots in reverse order (max to min). This causes us to add second and later slots to excess.
                            for reverse_slots in [false, true].iter().cloned() {
                                let maybe_reverse = |slot| {
                                    if reverse_slots {
                                        max - slot
                                    } else {
                                        slot
                                    }
                                };
                                if recreate {
                                    let recreated = setup_empty(width);
                                    tester = recreated;
                                }

                                // insert
                                for slot in slot..=slot2 {
                                    let slot_use = maybe_reverse(slot);
                                    tester.insert(slot_use);
                                    /*
                                    this is noisy on build machine
                                    debug!(
                                        "slot: {}, bitfield: {:?}, reverse: {}, len: {}, excess: {:?}",
                                        slot_use,
                                        tester.bitfield,
                                        reverse_slots,
                                        tester.bitfield.len(),
                                        tester.bitfield.excess
                                    );*/
                                    assert!(
                                        (reverse_slots && tester.bitfield.len() > 1)
                                            ^ tester.bitfield.excess.is_empty()
                                    );
                                }
                                if start > width * 2 {
                                    assert!(!tester.bitfield.contains(&(start - width * 2)));
                                }
                                assert!(!tester.bitfield.contains(&(start + width * 2)));
                                let len = (slot2 - slot + 1) as usize;
                                assert_eq!(tester.bitfield.len(), len);
                                assert_eq!(tester.bitfield.count, len);

                                // remove
                                for slot in slot..=slot2 {
                                    let slot_use = maybe_reverse(slot);
                                    assert!(tester.remove(&slot_use));
                                    assert!(
                                        (reverse_slots && !tester.bitfield.is_empty())
                                            ^ tester.bitfield.excess.is_empty()
                                    );
                                }
                                assert!(tester.bitfield.is_empty());
                                assert_eq!(tester.bitfield.count, 0);
                                if start > width * 2 {
                                    assert!(!tester.bitfield.contains(&(start - width * 2)));
                                }
                                assert!(!tester.bitfield.contains(&(start + width * 2)));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_bitfield_remove_wide_before() {
        let width = 16;
        let start = 100;
        let mut tester = setup_wide(width, start);
        let slot = start + 1 - width;
        assert!(!tester.remove(&slot));
    }

    fn compare_internal(hashset: &HashSet<u64>, bitfield: &RollingBitField) {
        assert_eq!(hashset.len(), bitfield.len());
        assert_eq!(hashset.is_empty(), bitfield.is_empty());
        if !bitfield.is_empty() {
            let mut min = Slot::MAX;
            let mut overall_min = Slot::MAX;
            let mut max = Slot::MIN;
            for item in bitfield.get_all() {
                assert!(hashset.contains(&item));
                if !bitfield.excess.contains(&item) {
                    min = std::cmp::min(min, item);
                    max = std::cmp::max(max, item);
                }
                overall_min = std::cmp::min(overall_min, item);
            }
            assert_eq!(bitfield.min(), Some(overall_min));
            assert_eq!(bitfield.get_all().len(), hashset.len());
            // range isn't tracked for excess items
            if bitfield.excess.len() != bitfield.len() {
                let width = if bitfield.is_empty() {
                    0
                } else {
                    max + 1 - min
                };
                assert!(
                    bitfield.range_width() >= width,
                    "hashset: {:?}, bitfield: {:?}, bitfield.range_width: {}, width: {}",
                    hashset,
                    bitfield.get_all(),
                    bitfield.range_width(),
                    width,
                );
            }
        } else {
            assert_eq!(bitfield.min(), None);
        }
    }

    fn compare(hashset: &HashSet<u64>, bitfield: &RollingBitField) {
        compare_internal(hashset, bitfield);
        let clone = bitfield.clone();
        compare_internal(hashset, &clone);
        assert!(clone.eq(bitfield));
        assert_eq!(clone, *bitfield);
    }

    #[test]
    fn test_bitfield_functionality() {
        solana_logger::setup();

        // bitfield sizes are powers of 2, cycle through values of 1, 2, 4, .. 2^9
        for power in 0..10 {
            let max_bitfield_width = 2u64.pow(power);
            let width_iteration_max = if max_bitfield_width > 1 {
                // add up to 2 items so we can test out multiple items
                3
            } else {
                // 0 or 1 items is all we can fit with a width of 1 item
                2
            };
            for width in 0..width_iteration_max {
                let mut tester = setup_empty(max_bitfield_width);

                let min = 101_000;
                let dead = 19;

                let mut slot = min;
                while tester.hash_set.len() < width {
                    slot += 1;
                    if max_bitfield_width > 2 && slot % dead == 0 {
                        // with max_bitfield_width of 1 and 2, there is no room for dead slots
                        continue;
                    }
                    tester.insert(slot);
                }
                let max = slot + 1;

                for slot in (min - 10)..max + 100 {
                    assert_eq!(
                        tester.bitfield.contains(&slot),
                        tester.hash_set.contains(&slot)
                    );
                }

                if width > 0 {
                    assert!(tester.remove(&slot));
                    assert!(!tester.remove(&slot));
                }

                let all = tester.bitfield.get_all();

                // remove the rest, including a call that removes slot again
                for item in all.iter() {
                    assert!(tester.remove(item));
                    assert!(!tester.remove(item));
                }

                let min = max + ((width * 2) as u64) + 3;
                let slot = min; // several widths past previous min
                let max = slot + 1;
                tester.insert(slot);

                for slot in (min - 10)..max + 100 {
                    assert_eq!(
                        tester.bitfield.contains(&slot),
                        tester.hash_set.contains(&slot)
                    );
                }
            }
        }
    }

    fn bitfield_insert_and_test(bitfield: &mut RollingBitField, slot: Slot) {
        let len = bitfield.len();
        let old_all = bitfield.get_all();
        let (new_min, new_max) = if bitfield.is_empty() {
            (slot, slot + 1)
        } else {
            (
                std::cmp::min(bitfield.min, slot),
                std::cmp::max(bitfield.max_exclusive, slot + 1),
            )
        };
        bitfield.insert(slot);
        assert_eq!(bitfield.min, new_min);
        assert_eq!(bitfield.max_exclusive, new_max);
        assert_eq!(bitfield.len(), len + 1);
        assert!(!bitfield.is_empty());
        assert!(bitfield.contains(&slot));
        // verify aliasing is what we expect
        assert!(bitfield.contains_assume_in_range(&(slot + bitfield.max_width)));
        let get_all = bitfield.get_all();
        old_all
            .into_iter()
            .for_each(|slot| assert!(get_all.contains(&slot)));
        assert!(get_all.contains(&slot));
        assert!(get_all.len() == len + 1);
    }

    #[test]
    fn test_bitfield_clear() {
        let mut bitfield = RollingBitField::new(4);
        assert_eq!(bitfield.len(), 0);
        assert!(bitfield.is_empty());
        bitfield_insert_and_test(&mut bitfield, 0);
        bitfield.clear();
        assert_eq!(bitfield.len(), 0);
        assert!(bitfield.is_empty());
        assert!(bitfield.get_all().is_empty());
        bitfield_insert_and_test(&mut bitfield, 1);
        bitfield.clear();
        assert_eq!(bitfield.len(), 0);
        assert!(bitfield.is_empty());
        assert!(bitfield.get_all().is_empty());
        bitfield_insert_and_test(&mut bitfield, 4);
    }

    #[test]
    fn test_bitfield_wrapping() {
        let mut bitfield = RollingBitField::new(4);
        assert_eq!(bitfield.len(), 0);
        assert!(bitfield.is_empty());
        bitfield_insert_and_test(&mut bitfield, 0);
        assert_eq!(bitfield.get_all(), vec![0]);
        bitfield_insert_and_test(&mut bitfield, 2);
        assert_eq!(bitfield.get_all(), vec![0, 2]);
        bitfield_insert_and_test(&mut bitfield, 3);
        bitfield.insert(3); // redundant insert
        assert_eq!(bitfield.get_all(), vec![0, 2, 3]);
        assert!(bitfield.remove(&0));
        assert!(!bitfield.remove(&0));
        assert_eq!(bitfield.min, 2);
        assert_eq!(bitfield.max_exclusive, 4);
        assert_eq!(bitfield.len(), 2);
        assert!(!bitfield.remove(&0)); // redundant remove
        assert_eq!(bitfield.len(), 2);
        assert_eq!(bitfield.get_all(), vec![2, 3]);
        bitfield.insert(4); // wrapped around value - same bit as '0'
        assert_eq!(bitfield.min, 2);
        assert_eq!(bitfield.max_exclusive, 5);
        assert_eq!(bitfield.len(), 3);
        assert_eq!(bitfield.get_all(), vec![2, 3, 4]);
        assert!(bitfield.remove(&2));
        assert_eq!(bitfield.min, 3);
        assert_eq!(bitfield.max_exclusive, 5);
        assert_eq!(bitfield.len(), 2);
        assert_eq!(bitfield.get_all(), vec![3, 4]);
        assert!(bitfield.remove(&3));
        assert_eq!(bitfield.min, 4);
        assert_eq!(bitfield.max_exclusive, 5);
        assert_eq!(bitfield.len(), 1);
        assert_eq!(bitfield.get_all(), vec![4]);
        assert!(bitfield.remove(&4));
        assert_eq!(bitfield.len(), 0);
        assert!(bitfield.is_empty());
        assert!(bitfield.get_all().is_empty());
        bitfield_insert_and_test(&mut bitfield, 8);
        assert!(bitfield.remove(&8));
        assert_eq!(bitfield.len(), 0);
        assert!(bitfield.is_empty());
        assert!(bitfield.get_all().is_empty());
        bitfield_insert_and_test(&mut bitfield, 9);
        assert!(bitfield.remove(&9));
        assert_eq!(bitfield.len(), 0);
        assert!(bitfield.is_empty());
        assert!(bitfield.get_all().is_empty());
    }

    #[test]
    fn test_bitfield_smaller() {
        // smaller bitfield, fewer entries, including 0
        solana_logger::setup();

        for width in 0..34 {
            let mut bitfield = RollingBitField::new(4096);
            let mut hash_set = HashSet::new();

            let min = 1_010_000;
            let dead = 19;

            let mut slot = min;
            while hash_set.len() < width {
                slot += 1;
                if slot % dead == 0 {
                    continue;
                }
                hash_set.insert(slot);
                bitfield.insert(slot);
            }

            let max = slot + 1;

            let mut time = Measure::start("");
            let mut count = 0;
            for slot in (min - 10)..max + 100 {
                if hash_set.contains(&slot) {
                    count += 1;
                }
            }
            time.stop();

            let mut time2 = Measure::start("");
            let mut count2 = 0;
            for slot in (min - 10)..max + 100 {
                if bitfield.contains(&slot) {
                    count2 += 1;
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
