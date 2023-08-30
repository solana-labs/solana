//! The list of slot boundaries at which a hard fork should
//! occur.

#![cfg(feature = "full")]

use {
    byteorder::{ByteOrder, LittleEndian},
    solana_sdk::clock::Slot,
};

#[derive(Default, Clone, Debug, Deserialize, Serialize, AbiExample, PartialEq, Eq)]
pub struct HardForks {
    hard_forks: Vec<(Slot, usize)>,
}
impl HardForks {
    // Register a fork to occur at all slots >= `slot` with a parent slot < `slot`
    pub fn register(&mut self, new_slot: Slot) {
        if let Some(i) = self
            .hard_forks
            .iter()
            .position(|(slot, _)| *slot == new_slot)
        {
            self.hard_forks[i] = (new_slot, self.hard_forks[i].1.saturating_add(1));
        } else {
            self.hard_forks.push((new_slot, 1));
        }
        #[allow(clippy::stable_sort_primitive)]
        self.hard_forks.sort();
    }

    // Returns a sorted-by-slot iterator over the registered hark forks
    pub fn iter(&self) -> std::slice::Iter<(Slot, usize)> {
        self.hard_forks.iter()
    }

    // Returns `true` is there are currently no registered hard forks
    pub fn is_empty(&self) -> bool {
        self.hard_forks.is_empty()
    }

    // Returns data to include in the bank hash for the given slot if a hard fork is scheduled
    pub fn get_hash_data(&self, slot: Slot, parent_slot: Slot) -> Option<[u8; 8]> {
        // The expected number of hard forks in a cluster is small.
        // If this turns out to be false then a more efficient data
        // structure may be needed here to avoid this linear search
        let fork_count: usize = self
            .hard_forks
            .iter()
            .map(|(fork_slot, fork_count)| {
                if parent_slot < *fork_slot && slot >= *fork_slot {
                    *fork_count
                } else {
                    0
                }
            })
            .sum();

        if fork_count > 0 {
            let mut buf = [0u8; 8];
            LittleEndian::write_u64(&mut buf[..], fork_count as u64);
            Some(buf)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_is_sorted() {
        let mut hf = HardForks::default();
        hf.register(30);
        hf.register(20);
        hf.register(10);
        hf.register(20);

        assert_eq!(hf.hard_forks, vec![(10, 1), (20, 2), (30, 1)]);
    }

    #[test]
    fn multiple_hard_forks_since_parent() {
        let mut hf = HardForks::default();
        hf.register(10);
        hf.register(20);

        assert_eq!(hf.get_hash_data(9, 0), None);
        assert_eq!(hf.get_hash_data(10, 0), Some([1, 0, 0, 0, 0, 0, 0, 0,]));
        assert_eq!(hf.get_hash_data(19, 0), Some([1, 0, 0, 0, 0, 0, 0, 0,]));
        assert_eq!(hf.get_hash_data(20, 0), Some([2, 0, 0, 0, 0, 0, 0, 0,]));
        assert_eq!(hf.get_hash_data(20, 10), Some([1, 0, 0, 0, 0, 0, 0, 0,]));
        assert_eq!(hf.get_hash_data(20, 11), Some([1, 0, 0, 0, 0, 0, 0, 0,]));
        assert_eq!(hf.get_hash_data(21, 11), Some([1, 0, 0, 0, 0, 0, 0, 0,]));
        assert_eq!(hf.get_hash_data(21, 20), None);
    }
}
