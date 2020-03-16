//!
//! slot history
//!
pub use crate::clock::Slot;
use bv::BitVec;

#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, Debug)]
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
        for skipped in self.next_slot..slot {
            self.bits.set(skipped % MAX_ENTRIES, false);
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

    #[test]
    fn test() {
        let mut slot_history = SlotHistory::default();
        slot_history.add(2);
        assert_eq!(slot_history.check(0), Check::Found);
        assert_eq!(slot_history.check(1), Check::NotFound);
        for i in 3..MAX_ENTRIES {
            assert_eq!(slot_history.check(i), Check::Future);
        }
        slot_history.add(MAX_ENTRIES);
        assert_eq!(slot_history.check(0), Check::TooOld);
        assert_eq!(slot_history.check(1), Check::NotFound);
        assert_eq!(slot_history.check(2), Check::Found);
    }

    #[test]
    fn test_bit_vec_deserialize() {
        use bincode::serialize_into;
        use std::io::{BufReader, Cursor};

        // an adhoc struct ABI-compatible with BitVec to fiddle the internal
        #[derive(Serialize)]
        struct Inner(Option<Box<[u64]>>);
        #[derive(Serialize)]
        struct LikeBitVec {
            bits: Inner,
            len: u64,
        }

        // normal
        let mut wr = Cursor::new(vec![]);
        let good = LikeBitVec {
            bits: Inner(Some(Box::new([0; 3]))),
            len: 15,
        };
        serialize_into(&mut wr, &good).unwrap();
        let buf = wr.into_inner();
        let d: Result<BitVec, _> = bincode::config()
            .limit(100)
            .deserialize_from(BufReader::new(&buf[..]));
        assert!(d.is_ok());
        assert_eq!(15, d.unwrap().len());

        // test large `bits` is guraded by `.limit(...)` just in case
        let mut wr = Cursor::new(vec![]);
        let large = LikeBitVec {
            bits: Inner(Some(Box::new([0; 30]))),
            len: 3,
        };
        serialize_into(&mut wr, &large).unwrap();
        let buf = wr.into_inner();
        let d: Result<BitVec, _> = bincode::config()
            .limit(100)
            .deserialize_from(BufReader::new(&buf[..]));
        assert!(d.is_err());

        // now test super huge mal-formed BitVec
        let mut wr = Cursor::new(vec![]);
        let bad = LikeBitVec {
            bits: Inner(Some(Box::new([0; 3]))),
            len: u64::max_value(),
        };
        serialize_into(&mut wr, &bad).unwrap();
        let buf = wr.into_inner();
        let d: Result<BitVec, _> = bincode::config()
            .limit(100)
            .deserialize_from(BufReader::new(&buf[..]));
        assert!(d.is_err());
    }
}
