use crate::erasure::{NUM_CODING, NUM_DATA};

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
// The Meta column family
pub struct SlotMeta {
    // The number of slots above the root (the genesis block). The first
    // slot has slot 0.
    pub slot: u64,
    // The total number of consecutive blobs starting from index 0
    // we have received for this slot.
    pub consumed: u64,
    // The index *plus one* of the highest blob received for this slot.  Useful
    // for checking if the slot has received any blobs yet, and to calculate the
    // range where there is one or more holes: `(consumed..received)`.
    pub received: u64,
    // The index of the blob that is flagged as the last blob for this slot.
    pub last_index: u64,
    // The slot height of the block this one derives from.
    pub parent_slot: u64,
    // The list of slot heights, each of which contains a block that derives
    // from this one.
    pub next_slots: Vec<u64>,
    // True if this slot is full (consumed == last_index + 1) and if every
    // slot that is a parent of this slot is also connected.
    pub is_connected: bool,
    // True if this slot is a root
    pub is_root: bool,
}

impl SlotMeta {
    pub fn is_full(&self) -> bool {
        // last_index is std::u64::MAX when it has no information about how
        // many blobs will fill this slot.
        // Note: A full slot with zero blobs is not possible.
        if self.last_index == std::u64::MAX {
            return false;
        }
        assert!(self.consumed <= self.last_index + 1);

        self.consumed == self.last_index + 1
    }

    pub fn is_parent_set(&self) -> bool {
        self.parent_slot != std::u64::MAX
    }

    pub(in crate::blocktree) fn new(slot: u64, parent_slot: u64) -> Self {
        SlotMeta {
            slot,
            consumed: 0,
            received: 0,
            parent_slot,
            next_slots: vec![],
            is_connected: slot == 0,
            is_root: false,
            last_index: std::u64::MAX,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
/// Erasure coding information
pub struct ErasureMeta {
    /// Which erasure set in the slot this is
    pub set_index: u64,
    /// Bitfield representing presence/absence of data blobs
    pub data: u64,
    /// Bitfield representing presence/absence of coding blobs
    pub coding: u64,
}

#[derive(Debug, PartialEq)]
pub enum ErasureMetaStatus {
    CanRecover,
    DataFull,
    StillNeed(usize),
}

impl ErasureMeta {
    pub fn new(set_index: u64) -> ErasureMeta {
        ErasureMeta {
            set_index,
            data: 0,
            coding: 0,
        }
    }

    pub fn status(&self) -> ErasureMetaStatus {
        let (data_missing, coding_missing) = (
            NUM_DATA - self.data.count_ones() as usize,
            NUM_CODING - self.coding.count_ones() as usize,
        );
        if data_missing > 0 && data_missing + coding_missing <= NUM_CODING {
            ErasureMetaStatus::CanRecover
        } else if data_missing == 0 {
            ErasureMetaStatus::DataFull
        } else {
            ErasureMetaStatus::StillNeed(data_missing + coding_missing - NUM_CODING)
        }
    }

    pub fn is_coding_present(&self, index: u64) -> bool {
        let start = self.start_index();
        let end = start + NUM_CODING as u64;

        if start <= index && index < end {
            let position = index - start;

            self.coding & (1 << position) != 0
        } else {
            false
        }
    }

    pub fn set_coding_present(&mut self, index: u64, present: bool) {
        let set_index = Self::set_index_for(index);

        if set_index as u64 == self.set_index {
            let position = index - self.start_index();

            if present {
                self.coding |= 1 << position;
            } else {
                self.coding &= !(1 << position);
            }
        }
    }

    pub fn is_data_present(&self, index: u64) -> bool {
        let start = self.start_index();
        let end = start + NUM_DATA as u64;

        if start <= index && index < end {
            let position = index - start;

            self.data & (1 << position) != 0
        } else {
            false
        }
    }

    pub fn set_data_present(&mut self, index: u64, present: bool) {
        let set_index = Self::set_index_for(index);

        if set_index as u64 == self.set_index {
            let position = index - self.start_index();

            if present {
                self.data |= 1 << position;
            } else {
                self.data &= !(1 << position);
            }
        }
    }

    pub fn set_index_for(index: u64) -> u64 {
        index / NUM_DATA as u64
    }

    pub fn start_index(&self) -> u64 {
        self.set_index * NUM_DATA as u64
    }

    /// returns a tuple of (data_end, coding_end)
    pub fn end_indexes(&self) -> (u64, u64) {
        let start = self.start_index();
        (start + NUM_DATA as u64, start + NUM_CODING as u64)
    }
}

#[test]
fn test_meta_indexes() {
    use rand::{thread_rng, Rng};

    let mut rng = thread_rng();

    for _ in 0..100 {
        let set_index = rng.gen_range(0, 1_000);
        let blob_index = (set_index * NUM_DATA as u64) + rng.gen_range(0, 16);

        assert_eq!(set_index, ErasureMeta::set_index_for(blob_index));
        let e_meta = ErasureMeta::new(set_index);

        assert_eq!(e_meta.start_index(), set_index * NUM_DATA as u64);
        let (data_end_idx, coding_end_idx) = e_meta.end_indexes();
        assert_eq!(data_end_idx, (set_index + 1) * NUM_DATA as u64);
        assert_eq!(
            coding_end_idx,
            set_index * NUM_DATA as u64 + NUM_CODING as u64
        );
    }
}

#[test]
fn test_meta_coding_present() {
    let set_index = 0;
    let mut e_meta = ErasureMeta {
        set_index,
        data: 0,
        coding: 0,
    };

    for i in 0..NUM_CODING as u64 {
        e_meta.set_coding_present(i, true);
        assert_eq!(e_meta.is_coding_present(i), true);
    }
    for i in NUM_CODING as u64..NUM_DATA as u64 {
        assert_eq!(e_meta.is_coding_present(i), false);
    }

    e_meta.set_index = ErasureMeta::set_index_for((NUM_DATA * 17) as u64);

    for i in (NUM_DATA * 17) as u64..((NUM_DATA * 17) + NUM_CODING) as u64 {
        e_meta.set_coding_present(i, true);
        assert_eq!(e_meta.is_coding_present(i), true);
    }
    for i in (NUM_DATA * 17 + NUM_CODING) as u64..((NUM_DATA * 17) + NUM_DATA) as u64 {
        assert_eq!(e_meta.is_coding_present(i), false);
    }
}

#[test]
fn test_erasure_meta_status() {
    let set_index = 0;
    let mut e_meta = ErasureMeta {
        set_index,
        data: 0,
        coding: 0,
    };

    assert_eq!(e_meta.status(), ErasureMetaStatus::StillNeed(NUM_DATA));

    e_meta.data = 0b1111_1111_1111_1111;
    e_meta.coding = 0x00;

    assert_eq!(e_meta.status(), ErasureMetaStatus::DataFull);

    e_meta.coding = 0x0e;
    assert_eq!(e_meta.status(), ErasureMetaStatus::DataFull);

    e_meta.data = 0b0111_1111_1111_1111;
    assert_eq!(e_meta.status(), ErasureMetaStatus::CanRecover);

    e_meta.data = 0b0111_1111_1111_1110;
    assert_eq!(e_meta.status(), ErasureMetaStatus::CanRecover);

    e_meta.data = 0b0111_1111_1011_1110;
    assert_eq!(e_meta.status(), ErasureMetaStatus::CanRecover);

    e_meta.data = 0b0111_1011_1011_1110;
    assert_eq!(e_meta.status(), ErasureMetaStatus::StillNeed(1));

    e_meta.data = 0b0111_1011_1011_1110;
    assert_eq!(e_meta.status(), ErasureMetaStatus::StillNeed(1));

    e_meta.coding = 0b0000_1110;
    e_meta.data = 0b1111_1111_1111_1100;
    assert_eq!(e_meta.status(), ErasureMetaStatus::CanRecover);

    e_meta.data = 0b1111_1111_1111_1000;
    assert_eq!(e_meta.status(), ErasureMetaStatus::CanRecover);
}

#[test]
fn test_meta_data_present() {
    let set_index = 0;
    let mut e_meta = ErasureMeta {
        set_index,
        data: 0,
        coding: 0,
    };

    for i in 0..NUM_DATA as u64 {
        e_meta.set_data_present(i, true);
        assert_eq!(e_meta.is_data_present(i), true);
    }
    for i in NUM_DATA as u64..2 * NUM_DATA as u64 {
        assert_eq!(e_meta.is_data_present(i), false);
    }

    e_meta.set_index = ErasureMeta::set_index_for((NUM_DATA * 23) as u64);

    for i in (NUM_DATA * 23) as u64..(NUM_DATA * 24) as u64 {
        e_meta.set_data_present(i, true);
        assert_eq!(e_meta.is_data_present(i), true);
    }
    for i in (NUM_DATA * 22) as u64..(NUM_DATA * 23) as u64 {
        assert_eq!(e_meta.is_data_present(i), false);
    }
}
