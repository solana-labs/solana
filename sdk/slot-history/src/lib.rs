//! A type to hold data for the [`SlotHistory` sysvar][sv].
//!
//! [sv]: https://docs.solanalabs.com/runtime/sysvars#slothistory
//!
//! The sysvar ID is declared in [`sysvar::slot_history`].
//!
//! [`sysvar::slot_history`]: https://docs.rs/solana-program/latest/solana_program/sysvar/slot_history

#![allow(clippy::arithmetic_side_effects)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "sysvar")]
pub mod sysvar;

use bv::{BitVec, BitsMut};

/// A bitvector indicating which slots are present in the past epoch.
#[repr(C)]
#[cfg_attr(
    feature = "serde",
    derive(serde_derive::Deserialize, serde_derive::Serialize)
)]
#[derive(Clone, PartialEq, Eq)]
pub struct SlotHistory {
    pub bits: BitVec<u64>,
    pub next_slot: u64,
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

#[derive(PartialEq, Eq, Debug)]
pub enum Check {
    Future,
    TooOld,
    Found,
    NotFound,
}

impl SlotHistory {
    pub fn add(&mut self, slot: u64) {
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

    pub fn check(&self, slot: u64) -> Check {
        if slot > self.newest() {
            Check::Future
        } else if slot < self.oldest() {
            Check::TooOld
        } else if self.bits.get(slot % MAX_ENTRIES) {
            Check::Found
        } else {
            Check::NotFound
        }
    }

    pub fn oldest(&self) -> u64 {
        self.next_slot.saturating_sub(MAX_ENTRIES)
    }

    pub fn newest(&self) -> u64 {
        self.next_slot - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_history_test1() {
        // should be divisible by 64 since the clear logic works on blocks
        assert_eq!(MAX_ENTRIES % 64, 0);
        let mut slot_history = SlotHistory::default();
        slot_history.add(2);
        assert_eq!(slot_history.check(0), Check::Found);
        assert_eq!(slot_history.check(1), Check::NotFound);
        for i in 3..MAX_ENTRIES {
            assert_eq!(slot_history.check(i), Check::Future);
        }
        slot_history.add(20);
        slot_history.add(MAX_ENTRIES);
        assert_eq!(slot_history.check(0), Check::TooOld);
        assert_eq!(slot_history.check(1), Check::NotFound);
        for i in &[2, 20, MAX_ENTRIES] {
            assert_eq!(slot_history.check(*i), Check::Found);
        }
        for i in 3..20 {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {i}");
        }
        for i in 21..MAX_ENTRIES {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {i}");
        }
        assert_eq!(slot_history.check(MAX_ENTRIES + 1), Check::Future);

        let slot = 3 * MAX_ENTRIES + 3;
        slot_history.add(slot);
        for i in &[0, 1, 2, 20, 21, MAX_ENTRIES] {
            assert_eq!(slot_history.check(*i), Check::TooOld);
        }
        let start = slot - MAX_ENTRIES + 1;
        let end = slot;
        for i in start..end {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {i}");
        }
        assert_eq!(slot_history.check(slot), Check::Found);
    }

    #[test]
    fn slot_history_test_wrap() {
        let mut slot_history = SlotHistory::default();
        slot_history.add(2);
        assert_eq!(slot_history.check(0), Check::Found);
        assert_eq!(slot_history.check(1), Check::NotFound);
        for i in 3..MAX_ENTRIES {
            assert_eq!(slot_history.check(i), Check::Future);
        }
        slot_history.add(20);
        slot_history.add(MAX_ENTRIES + 19);
        for i in 0..19 {
            assert_eq!(slot_history.check(i), Check::TooOld);
        }
        assert_eq!(slot_history.check(MAX_ENTRIES), Check::NotFound);
        assert_eq!(slot_history.check(20), Check::Found);
        assert_eq!(slot_history.check(MAX_ENTRIES + 19), Check::Found);
        assert_eq!(slot_history.check(20), Check::Found);
        for i in 21..MAX_ENTRIES + 19 {
            assert_eq!(slot_history.check(i), Check::NotFound, "found: {i}");
        }
        assert_eq!(slot_history.check(MAX_ENTRIES + 20), Check::Future);
    }

    #[test]
    fn slot_history_test_same_index() {
        let mut slot_history = SlotHistory::default();
        slot_history.add(3);
        slot_history.add(4);
        assert_eq!(slot_history.check(1), Check::NotFound);
        assert_eq!(slot_history.check(2), Check::NotFound);
        assert_eq!(slot_history.check(3), Check::Found);
        assert_eq!(slot_history.check(4), Check::Found);
        slot_history.add(MAX_ENTRIES + 5);
        assert_eq!(slot_history.check(5), Check::TooOld);
        for i in 6..MAX_ENTRIES + 5 {
            assert_eq!(slot_history.check(i), Check::NotFound, "i: {i}");
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

    #[test]
    fn test_oldest() {
        let mut slot_history = SlotHistory::default();
        assert_eq!(slot_history.oldest(), 0);
        slot_history.add(10);
        assert_eq!(slot_history.oldest(), 0);
        slot_history.add(MAX_ENTRIES - 1);
        assert_eq!(slot_history.oldest(), 0);
        slot_history.add(MAX_ENTRIES);
        assert_eq!(slot_history.oldest(), 1);
    }
}
