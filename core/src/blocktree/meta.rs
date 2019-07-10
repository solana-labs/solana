use crate::erasure::ErasureConfig;
use solana_metrics::datapoint;
use std::{collections::BTreeSet, ops::RangeBounds};

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
    /// Map representing presence/absence of data blobs
    index: BTreeSet<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
/// Erasure coding information
pub struct CodingIndex {
    /// Map from set index, to hashmap from blob index to presence bool
    index: BTreeSet<u64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
/// Erasure coding information
pub struct ErasureMeta {
    /// Which erasure set in the slot this is
    pub set_index: u64,
    /// Size of shards in this erasure set
    pub size: usize,
    /// Erasure configuration for this erasure set
    config: ErasureConfig,
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

/// TODO: Mark: Change this when coding
impl CodingIndex {
    pub fn present_in_bounds(&self, bounds: impl RangeBounds<u64>) -> usize {
        self.index.range(bounds).count()
    }

    pub fn is_present(&self, index: u64) -> bool {
        self.index.contains(&index)
    }

    pub fn set_present(&mut self, index: u64, presence: bool) {
        if presence {
            self.index.insert(index);
        } else {
            self.index.remove(&index);
        }
    }

    pub fn set_many_present(&mut self, presence: impl IntoIterator<Item = (u64, bool)>) {
        for (idx, present) in presence.into_iter() {
            self.set_present(idx, present);
        }
    }
}

impl DataIndex {
    pub fn present_in_bounds(&self, bounds: impl RangeBounds<u64>) -> usize {
        self.index.range(bounds).count()
    }

    pub fn is_present(&self, index: u64) -> bool {
        self.index.contains(&index)
    }

    pub fn set_present(&mut self, index: u64, presence: bool) {
        if presence {
            self.index.insert(index);
        } else {
            self.index.remove(&index);
        }
    }

    pub fn set_many_present(&mut self, presence: impl IntoIterator<Item = (u64, bool)>) {
        for (idx, present) in presence.into_iter() {
            self.set_present(idx, present);
        }
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
    pub fn new(set_index: u64, config: &ErasureConfig) -> ErasureMeta {
        ErasureMeta {
            set_index,
            size: 0,
            config: *config,
        }
    }

    pub fn status(&self, index: &Index) -> ErasureMetaStatus {
        use ErasureMetaStatus::*;

        let start_idx = self.start_index();
        let (data_end_idx, coding_end_idx) = self.end_indexes();

        let num_coding = index.coding().present_in_bounds(start_idx..coding_end_idx);
        let num_data = index.data().present_in_bounds(start_idx..data_end_idx);

        let (data_missing, coding_missing) = (
            self.config.num_data() - num_data,
            self.config.num_coding() - num_coding,
        );

        let total_missing = data_missing + coding_missing;

        if data_missing > 0 && total_missing <= self.config.num_coding() {
            CanRecover
        } else if data_missing == 0 {
            DataFull
        } else {
            StillNeed(total_missing - self.config.num_coding())
        }
    }

    pub fn set_size(&mut self, size: usize) {
        self.size = size;
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn set_index_for(index: u64, num_data: usize) -> u64 {
        index / num_data as u64
    }

    pub fn start_index(&self) -> u64 {
        self.set_index * self.config.num_data() as u64
    }

    /// returns a tuple of (data_end, coding_end)
    pub fn end_indexes(&self) -> (u64, u64) {
        let start = self.start_index();
        (
            start + self.config.num_data() as u64,
            start + self.config.num_coding() as u64,
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::{seq::SliceRandom, thread_rng};
    use std::iter::repeat;

    #[test]
    fn test_erasure_meta_status() {
        use ErasureMetaStatus::*;

        let set_index = 0;
        let erasure_config = ErasureConfig::default();

        let mut e_meta = ErasureMeta::new(set_index, &erasure_config);
        let mut rng = thread_rng();
        let mut index = Index::new(0);
        e_meta.size = 1;

        let data_indexes = 0..erasure_config.num_data() as u64;
        let coding_indexes = 0..erasure_config.num_coding() as u64;

        assert_eq!(e_meta.status(&index), StillNeed(erasure_config.num_data()));

        index
            .data_mut()
            .set_many_present(data_indexes.clone().zip(repeat(true)));

        assert_eq!(e_meta.status(&index), DataFull);

        index
            .coding_mut()
            .set_many_present(coding_indexes.clone().zip(repeat(true)));

        for &idx in data_indexes
            .clone()
            .collect::<Vec<_>>()
            .choose_multiple(&mut rng, erasure_config.num_data())
        {
            index.data_mut().set_present(idx, false);

            assert_eq!(e_meta.status(&index), CanRecover);
        }

        index
            .data_mut()
            .set_many_present(data_indexes.zip(repeat(true)));

        for &idx in coding_indexes
            .collect::<Vec<_>>()
            .choose_multiple(&mut rng, erasure_config.num_coding())
        {
            index.coding_mut().set_present(idx, false);

            assert_eq!(e_meta.status(&index), DataFull);
        }
    }
}
