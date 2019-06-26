use crate::erasure::{NUM_CODING, NUM_DATA};
use solana_metrics::datapoint;
use std::{collections::BTreeMap, ops::RangeBounds};

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
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
/// Index recording presence/absence of blobs
pub struct Index {
    pub slot: u64,
    data: DataIndex,
    coding: CodingIndex,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct DataIndex {
    slot: u64,
    /// Map representing presence/absence of data blobs
    index: BTreeMap<u64, bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
/// Erasure coding information
pub struct CodingIndex {
    slot: u64,
    /// Map from set index, to hashmap from blob index to presence bool
    index: BTreeMap<u64, bool>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
/// Erasure coding information
pub struct ErasureMeta {
    /// Which erasure set in the slot this is
    pub set_index: u64,
    /// Size of shards in this erasure set
    size: usize,
}

#[derive(Debug, PartialEq)]
pub enum ErasureMetaStatus {
    CanRecover,
    DataFull,
    StillNeed(usize),
}

impl Index {
    pub(in crate::blocktree) fn new(slot: u64) -> Self {
        Index {
            slot,
            data: DataIndex::default(),
            coding: CodingIndex::default(),
        }
    }

    pub fn data(&self) -> &DataIndex {
        &self.data
    }
    pub fn coding(&self) -> &CodingIndex {
        &self.coding
    }

    pub fn data_mut(&mut self) -> &mut DataIndex {
        &mut self.data
    }
    pub fn coding_mut(&mut self) -> &mut CodingIndex {
        &mut self.coding
    }
}

impl CodingIndex {
    pub fn present_in_bounds(&self, bounds: impl RangeBounds<u64>) -> usize {
        self.index
            .range(bounds)
            .filter(|(_, presence)| **presence)
            .count()
    }

    pub fn is_present(&self, index: u64) -> bool {
        *self.index.get(&index).unwrap_or(&false)
    }

    pub fn set_present(&mut self, index: u64, presence: bool) {
        self.index.insert(index, presence);
    }
}

impl DataIndex {
    pub fn present_in_bounds(&self, bounds: impl RangeBounds<u64>) -> usize {
        self.index
            .range(bounds)
            .filter(|(_, presence)| **presence)
            .count()
    }

    pub fn is_present(&self, index: u64) -> bool {
        *self.index.get(&index).unwrap_or(&false)
    }

    pub fn set_present(&mut self, index: u64, presence: bool) {
        self.index.insert(index, presence);
    }
}

impl SlotMeta {
    pub fn is_full(&self) -> bool {
        // last_index is std::u64::MAX when it has no information about how
        // many blobs will fill this slot.
        // Note: A full slot with zero blobs is not possible.
        if self.last_index == std::u64::MAX {
            return false;
        }

        // Should never happen
        if self.consumed > self.last_index + 1 {
            datapoint!(
                "blocktree_error",
                (
                    "error",
                    format!(
                        "Observed a slot meta with consumed: {} > meta.last_index + 1: {}",
                        self.consumed,
                        self.last_index + 1
                    ),
                    String
                )
            );
        }

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
            last_index: std::u64::MAX,
        }
    }
}

impl ErasureMeta {
    pub fn new(set_index: u64) -> ErasureMeta {
        ErasureMeta { set_index, size: 0 }
    }

    pub fn status(&self, index: &Index) -> ErasureMetaStatus {
        use ErasureMetaStatus::*;

        let start_idx = self.start_index();
        let (data_end_idx, coding_end_idx) = self.end_indexes();

        let num_coding = index.coding().present_in_bounds(start_idx..coding_end_idx);
        let num_data = index.data().present_in_bounds(start_idx..data_end_idx);

        let (data_missing, coding_missing) = (NUM_DATA - num_data, NUM_CODING - num_coding);

        let total_missing = data_missing + coding_missing;

        if data_missing > 0 && total_missing <= NUM_CODING {
            CanRecover
        } else if data_missing == 0 {
            DataFull
        } else {
            StillNeed(total_missing - NUM_CODING)
        }
    }

    pub fn set_size(&mut self, size: usize) {
        self.size = size;
    }

    pub fn size(&self) -> usize {
        self.size
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

#[cfg(test)]
mod test {
    use super::*;

    const NUM_DATA: u64 = super::NUM_DATA as u64;
    const NUM_CODING: u64 = super::NUM_CODING as u64;

    #[test]
    fn test_erasure_meta_status() {
        let set_index = 0;

        let mut e_meta = ErasureMeta::new(set_index);
        e_meta.size = 1;
        let mut index = Index::new(0);

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::StillNeed(8));

        for i in 0..NUM_DATA {
            index.data_mut().set_present(i, true);
        }

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::DataFull);

        index.data_mut().set_present(NUM_DATA - 1, false);

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::StillNeed(1));

        for i in 0..NUM_DATA - 2 {
            index.data_mut().set_present(i, false);
        }

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::StillNeed(7));

        for i in 0..NUM_CODING {
            index.coding_mut().set_present(i, true);
        }

        index.data_mut().set_present(NUM_DATA - 1, false);

        for i in 0..NUM_DATA - 1 {
            index.data_mut().set_present(i, true);

            assert_eq!(e_meta.status(&index), ErasureMetaStatus::CanRecover);
        }
    }
}
