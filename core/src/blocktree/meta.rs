use crate::erasure::{NUM_CODING, NUM_DATA};
use solana_metrics::datapoint;
use std::borrow::Borrow;

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

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
/// Erasure coding information
pub struct ErasureMeta {
    /// Which erasure set in the slot this is
    pub set_index: u64,
    /// Size of shards in this erasure set
    size: usize,
    /// Bitfield representing presence/absence of data blobs
    data: u64,
    /// Bitfield representing presence/absence of coding blobs
    coding: u64,
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
            size: 0,
            data: 0,
            coding: 0,
        }
    }

    pub fn status(&self) -> ErasureMetaStatus {
        let (data_missing, coding_missing) =
            (NUM_DATA - self.num_data(), NUM_CODING - self.num_coding());
        if data_missing > 0 && data_missing + coding_missing <= NUM_CODING {
            //assert!(self.size != 0);
            ErasureMetaStatus::CanRecover
        } else if data_missing == 0 {
            ErasureMetaStatus::DataFull
        } else {
            ErasureMetaStatus::StillNeed(data_missing + coding_missing - NUM_CODING)
        }
    }

    pub fn num_coding(&self) -> usize {
        self.coding.count_ones() as usize
    }

    pub fn num_data(&self) -> usize {
        self.data.count_ones() as usize
    }

    pub fn is_coding_present(&self, index: u64) -> bool {
        if let Some(position) = self.data_index_in_set(index) {
            self.coding & (1 << position) != 0
        } else {
            false
        }
    }

    pub fn set_size(&mut self, size: usize) {
        self.size = size;
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn set_coding_present(&mut self, index: u64, present: bool) {
        if let Some(position) = self.data_index_in_set(index) {
            if present {
                self.coding |= 1 << position;
            } else {
                self.coding &= !(1 << position);
            }
        }
    }

    pub fn is_data_present(&self, index: u64) -> bool {
        if let Some(position) = self.data_index_in_set(index) {
            self.data & (1 << position) != 0
        } else {
            false
        }
    }

    pub fn set_data_present(&mut self, index: u64, present: bool) {
        if let Some(position) = self.data_index_in_set(index) {
            if present {
                self.data |= 1 << position;
            } else {
                self.data &= !(1 << position);
            }
        }
    }

    pub fn set_data_multi<I, Idx>(&mut self, indexes: I, present: bool)
    where
        I: IntoIterator<Item = Idx>,
        Idx: Borrow<u64>,
    {
        for index in indexes.into_iter() {
            self.set_data_present(*index.borrow(), present);
        }
    }

    pub fn set_coding_multi<I, Idx>(&mut self, indexes: I, present: bool)
    where
        I: IntoIterator<Item = Idx>,
        Idx: Borrow<u64>,
    {
        for index in indexes.into_iter() {
            self.set_coding_present(*index.borrow(), present);
        }
    }

    pub fn set_index_for(index: u64) -> u64 {
        index / NUM_DATA as u64
    }

    pub fn data_index_in_set(&self, index: u64) -> Option<u64> {
        let set_index = Self::set_index_for(index);

        if set_index == self.set_index {
            Some(index - self.start_index())
        } else {
            None
        }
    }

    pub fn coding_index_in_set(&self, index: u64) -> Option<u64> {
        self.data_index_in_set(index).map(|i| i + NUM_DATA as u64)
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
    // to avoid casts everywhere
    const NUM_DATA: u64 = crate::erasure::NUM_DATA as u64;

    let mut rng = thread_rng();

    for _ in 0..100 {
        let set_index = rng.gen_range(0, 1_000);
        let blob_index = (set_index * NUM_DATA) + rng.gen_range(0, NUM_DATA);

        assert_eq!(set_index, ErasureMeta::set_index_for(blob_index));
        let e_meta = ErasureMeta::new(set_index);

        assert_eq!(e_meta.start_index(), set_index * NUM_DATA);
        let (data_end_idx, coding_end_idx) = e_meta.end_indexes();
        assert_eq!(data_end_idx, (set_index + 1) * NUM_DATA);
        assert_eq!(coding_end_idx, set_index * NUM_DATA + NUM_CODING as u64);
    }

    let mut e_meta = ErasureMeta::new(0);

    assert_eq!(e_meta.data_index_in_set(0), Some(0));
    assert_eq!(e_meta.data_index_in_set(NUM_DATA / 2), Some(NUM_DATA / 2));
    assert_eq!(e_meta.data_index_in_set(NUM_DATA - 1), Some(NUM_DATA - 1));
    assert_eq!(e_meta.data_index_in_set(NUM_DATA), None);
    assert_eq!(e_meta.data_index_in_set(std::u64::MAX), None);

    e_meta.set_index = 1;

    assert_eq!(e_meta.data_index_in_set(0), None);
    assert_eq!(e_meta.data_index_in_set(NUM_DATA - 1), None);
    assert_eq!(e_meta.data_index_in_set(NUM_DATA), Some(0));
    assert_eq!(
        e_meta.data_index_in_set(NUM_DATA * 2 - 1),
        Some(NUM_DATA - 1)
    );
    assert_eq!(e_meta.data_index_in_set(std::u64::MAX), None);
}

#[test]
fn test_meta_coding_present() {
    let mut e_meta = ErasureMeta::default();

    e_meta.set_coding_multi(0..NUM_CODING as u64, true);
    for i in 0..NUM_CODING as u64 {
        assert_eq!(e_meta.is_coding_present(i), true);
    }
    for i in NUM_CODING as u64..NUM_DATA as u64 {
        assert_eq!(e_meta.is_coding_present(i), false);
    }

    e_meta.set_index = ErasureMeta::set_index_for((NUM_DATA * 17) as u64);
    let start_idx = e_meta.start_index();
    e_meta.set_coding_multi(start_idx..start_idx + NUM_CODING as u64, true);

    for i in start_idx..start_idx + NUM_CODING as u64 {
        e_meta.set_coding_present(i, true);
        assert_eq!(e_meta.is_coding_present(i), true);
    }
    for i in start_idx + NUM_CODING as u64..start_idx + NUM_DATA as u64 {
        assert_eq!(e_meta.is_coding_present(i), false);
    }
}

#[test]
fn test_erasure_meta_status() {
    use rand::{seq::SliceRandom, thread_rng};
    // Local constansts just used to avoid repetitive casts
    const N_DATA: u64 = crate::erasure::NUM_DATA as u64;
    const N_CODING: u64 = crate::erasure::NUM_CODING as u64;

    let mut e_meta = ErasureMeta::default();
    let mut rng = thread_rng();
    let data_indexes: Vec<u64> = (0..N_DATA).collect();
    let coding_indexes: Vec<u64> = (0..N_CODING).collect();

    assert_eq!(e_meta.status(), ErasureMetaStatus::StillNeed(NUM_DATA));

    e_meta.set_data_multi(0..N_DATA, true);

    assert_eq!(e_meta.status(), ErasureMetaStatus::DataFull);

    e_meta.size = 1;
    e_meta.set_coding_multi(0..N_CODING, true);

    assert_eq!(e_meta.status(), ErasureMetaStatus::DataFull);

    for &idx in data_indexes.choose_multiple(&mut rng, NUM_CODING) {
        e_meta.set_data_present(idx, false);

        assert_eq!(e_meta.status(), ErasureMetaStatus::CanRecover);
    }

    e_meta.set_data_multi(0..N_DATA, true);

    for &idx in coding_indexes.choose_multiple(&mut rng, NUM_CODING) {
        e_meta.set_coding_present(idx, false);

        assert_eq!(e_meta.status(), ErasureMetaStatus::DataFull);
    }
}

#[test]
fn test_meta_data_present() {
    let mut e_meta = ErasureMeta::default();

    e_meta.set_data_multi(0..NUM_DATA as u64, true);
    for i in 0..NUM_DATA as u64 {
        assert_eq!(e_meta.is_data_present(i), true);
    }
    for i in NUM_DATA as u64..2 * NUM_DATA as u64 {
        assert_eq!(e_meta.is_data_present(i), false);
    }

    e_meta.set_index = ErasureMeta::set_index_for((NUM_DATA * 23) as u64);
    let start_idx = e_meta.start_index();
    e_meta.set_data_multi(start_idx..start_idx + NUM_DATA as u64, true);

    for i in start_idx..start_idx + NUM_DATA as u64 {
        assert_eq!(e_meta.is_data_present(i), true);
    }
    for i in start_idx - NUM_DATA as u64..start_idx {
        assert_eq!(e_meta.is_data_present(i), false);
    }
}
