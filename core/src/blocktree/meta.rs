use crate::erasure::CodingHeader;
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
    index: BTreeMap<u64, BTreeMap<u64, bool>>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
/// Erasure coding information
pub struct ErasureMeta {
    header: CodingHeader,
    set_index: u64,
}

#[derive(Debug, PartialEq)]
pub enum ErasureMetaStatus {
    CanRecover,
    DataFull,
    StillNeed(usize),
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
    pub fn is_set_present(&self, set_index: u64) -> bool {
        self.index.contains_key(&set_index)
    }

    pub fn present_in_set(&self, set_index: u64) -> usize {
        match self.index.get(&set_index) {
            Some(map) => map.values().filter(|presence| **presence).count(),
            None => 0,
        }
    }

    pub fn is_present(&self, set_index: u64, blob_index: u64) -> bool {
        match self.index.get(&set_index) {
            Some(map) => *map.get(&blob_index).unwrap_or(&false),
            None => false,
        }
    }

    pub fn set_present(&mut self, set_index: u64, blob_index: u64, present: bool) {
        let set_map = self
            .index
            .entry(set_index)
            .or_insert_with(BTreeMap::default);

        set_map.insert(blob_index, present);
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

impl ErasureMeta {
    pub(in crate::blocktree) fn new(set_index: u64) -> ErasureMeta {
        ErasureMeta {
            header: CodingHeader::default(),
            set_index,
        }
    }

    pub fn session_info(&self) -> CodingHeader {
        self.header
    }

    pub fn set_session_info(&mut self, header: CodingHeader) {
        self.header = header;
    }

    pub fn status(&self, index: &Index) -> ErasureMetaStatus {
        use ErasureMetaStatus::*;

        let start_idx = self.header.start_index;
        let end_idx = start_idx + self.header.data_count as u64;

        let num_coding = index.coding().present_in_set(self.header.set_index);
        let num_data = index.data().present_in_bounds(start_idx..end_idx);

        assert!(self.header.shard_size != 0);

        let (data_missing, coding_missing) = (
            self.header.data_count - num_data,
            self.header.parity_count - num_coding,
        );

        let total_missing = data_missing + coding_missing;

        if data_missing > 0 && total_missing <= self.header.parity_count {
            CanRecover
        } else if data_missing == 0 {
            DataFull
        } else {
            StillNeed(total_missing - self.header.parity_count)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const NUM_DATA: u64 = 7;
    const NUM_CODING: u64 = 8;

    fn sample_header() -> CodingHeader {
        CodingHeader {
            shard_size: 1,
            data_count: NUM_DATA as usize,
            parity_count: NUM_CODING as usize,
            ..CodingHeader::default()
        }
    }

    #[test]
    fn test_erasure_meta_status() {
        let set_index = 0;

        let header = sample_header();
        let mut e_meta = ErasureMeta::new(set_index);
        e_meta.set_session_info(header);

        let mut index = Index::new(0);

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::StillNeed(7));

        for i in 0..NUM_DATA {
            index.data_mut().set_present(i, true);
        }

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::DataFull);

        index.data_mut().set_present(NUM_DATA - 1, false);

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::StillNeed(1));

        for i in 0..NUM_DATA - 2 {
            index.data_mut().set_present(i, false);
        }

        assert_eq!(e_meta.status(&index), ErasureMetaStatus::StillNeed(6));

        for i in 0..NUM_CODING {
            index.coding_mut().set_present(set_index, i, true);
        }

        index.data_mut().set_present(NUM_DATA - 1, false);

        for i in 0..NUM_DATA - 1 {
            index.data_mut().set_present(i, true);

            assert_eq!(e_meta.status(&index), ErasureMetaStatus::CanRecover);
        }
    }
}
