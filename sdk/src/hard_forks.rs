//! The `hard_forks` module is used to maintain the list of slot boundaries for when a hard fork
//! should occur.

use byteorder::{ByteOrder, LittleEndian};
use solana_sdk::clock::Slot;
use std::ops::Add;

#[derive(Default, Clone, Deserialize, Serialize)]
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
            self.hard_forks[i] = (new_slot, self.hard_forks[i].1 + 1);
        } else {
            self.hard_forks.push((new_slot, 1));
        }
        self.hard_forks.sort();
    }

    // Returns a sorted-by-slot iterator over the registered hark forks
    pub fn iter(&self) -> std::slice::Iter<(Slot, usize)> {
        self.hard_forks.iter()
    }

    // Returns data to include in the bank hash for the given slot if a hard fork is scheduled
    pub fn get_hash_data(&self, slot: Slot, parent_slot: Slot) -> Option<[u8; 8]> {
        // The expected number of hard forks in a cluster is small.
        // If this turns out to be false then a more efficient data
        // structure may be needed here to avoid this linear search
        let fork_count = self
            .hard_forks
            .iter()
            .fold(0, |acc, (fork_slot, fork_count)| {
                acc.add(if parent_slot < *fork_slot && slot >= *fork_slot {
                    *fork_count
                } else {
                    0
                })
            });

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
