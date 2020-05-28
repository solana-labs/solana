//!
//! slot history
//!
pub use crate::clock::Slot;
use bv::BitVec;
use bv::BitsMut;

#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq)]
pub struct SlotHistory {
    pub bits: BitVec<u64>,
    pub next_slot: Slot,
}

impl Default for SlotHistory {
    fn default() -> Self {
        let mut bits = BitVec::new_fill(false, MAX_ENTRIES);
        bits.set(0, true);
        Self { bits, next_slot: 1 }
    }
}

impl std::fmt::Debug for SlotHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SlotHistory {{ slot: {} bits:", self.next_slot)?;
        for i in 0..MAX_ENTRIES {
            if self.bits.get(i) {
                write!(f, "1")?;
            } else {
                write!(f, "0")?;
            }
        }
        Ok(())
    }
}

pub const MAX_ENTRIES: u64 = 1024 * 1024; // 1 million slots is about 5 days

#[derive(PartialEq, Debug)]
pub enum Check {
    Future,
    TooOld,
    Found,
    NotFound,
}

impl SlotHistory {
    pub fn add(&mut self, slot: Slot) {
        if slot > self.next_slot && slot - self.next_slot >= MAX_ENTRIES {
            // Wrapped past current history,
            // clear entire bitvec.
            let full_blocks = (MAX_ENTRIES as usize) / 64;
            for i in 0..full_blocks {
                self.bits.set_block(i, 0);
            }
        } else {
            for skipped in self.next_slot..slot {
                self.bits.set(skipped % MAX_ENTRIES, false);
            }
        }
        self.bits.set(slot % MAX_ENTRIES, true);
        self.next_slot = slot + 1;
    }

    pub fn check(&self, slot: Slot) -> Check {
        if slot >= self.next_slot {
            Check::Future
        } else if self.next_slot - slot > MAX_ENTRIES {
            Check::TooOld
        } else if self.bits.get(slot % MAX_ENTRIES) {
            Check::Found
        } else {
            Check::NotFound
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::*;

    #[test]
    fn slot_history_test1() {
        solana_logger::setup();
        // should be divisable by 64 since the clear logic works on blocks
        assert_eq!(MAX_ENTRIES % 64, 0);
        let mut slot_history = SlotHistory::default();
        info!("add 2");
        slot_history.add(2);
        assert_eq!(slot_history.check(0), Check::Found);
        assert_eq!(slot_history.check(1), Check::NotFound);
        for i in 3..MAX_ENTRIES {
            assert_eq!(slot_history.check(i), Check::Future);
        }
        info!("add 20");
        slot_history.add(20);
        info!("add max_entries");
        slot_history.add(MAX_ENTRIES);
        assert_eq!(slot_history.check(0), Check::TooOld);
        assert_eq!(slot_history.check(1), Check::NotFound);
        for i in &[2, 20, MAX_ENTRIES] {
            assert_eq!(slot_history.check(*i), Check::Found);
        }
        for i in 3..20 {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {}", i);
        }
        for i in 21..MAX_ENTRIES {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {}", i);
        }
        assert_eq!(slot_history.check(MAX_ENTRIES + 1), Check::Future);

        info!("add max_entries + 3");
        let slot = 3 * MAX_ENTRIES + 3;
        slot_history.add(slot);
        for i in &[0, 1, 2, 20, 21, MAX_ENTRIES] {
            assert_eq!(slot_history.check(*i), Check::TooOld);
        }
        let start = slot - MAX_ENTRIES + 1;
        let end = slot;
        for i in start..end {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {}", i);
        }
        assert_eq!(slot_history.check(slot), Check::Found);
    }

    #[test]
    fn slot_history_test_wrap() {
        solana_logger::setup();
        let mut slot_history = SlotHistory::default();
        info!("add 2");
        slot_history.add(2);
        assert_eq!(slot_history.check(0), Check::Found);
        assert_eq!(slot_history.check(1), Check::NotFound);
        for i in 3..MAX_ENTRIES {
            assert_eq!(slot_history.check(i), Check::Future);
        }
        info!("add 20");
        slot_history.add(20);
        info!("add max_entries + 19");
        slot_history.add(MAX_ENTRIES + 19);
        for i in 0..19 {
            assert_eq!(slot_history.check(i), Check::TooOld);
        }
        assert_eq!(slot_history.check(MAX_ENTRIES), Check::NotFound);
        assert_eq!(slot_history.check(20), Check::Found);
        assert_eq!(slot_history.check(MAX_ENTRIES + 19), Check::Found);
        assert_eq!(slot_history.check(20), Check::Found);
        for i in 21..MAX_ENTRIES + 19 {
            assert_eq!(slot_history.check(i), Check::NotFound, "found: {}", i);
        }
        assert_eq!(slot_history.check(MAX_ENTRIES + 20), Check::Future);
    }

    #[test]
    fn slot_history_test_same_index() {
        solana_logger::setup();
        let mut slot_history = SlotHistory::default();
        info!("add 3,4");
        slot_history.add(3);
        slot_history.add(4);
        assert_eq!(slot_history.check(1), Check::NotFound);
        assert_eq!(slot_history.check(2), Check::NotFound);
        assert_eq!(slot_history.check(3), Check::Found);
        assert_eq!(slot_history.check(4), Check::Found);
        slot_history.add(MAX_ENTRIES + 5);
        assert_eq!(slot_history.check(5), Check::TooOld);
        for i in 6..MAX_ENTRIES + 5 {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {}", i);
        }
        assert_eq!(slot_history.check(MAX_ENTRIES + 5), Check::Found);
    }

    #[test]
    fn test_older_slot() {
        let mut slot_history = SlotHistory::default();
        slot_history.add(10);
        slot_history.add(5);
        assert_eq!(slot_history.check(0), Check::Found);
        assert_eq!(slot_history.check(5), Check::Found);
        // If we go backwards we reset?
        assert_eq!(slot_history.check(10), Check::Future);
        assert_eq!(slot_history.check(6), Check::Future);
        assert_eq!(slot_history.check(11), Check::Future);
    }
}
