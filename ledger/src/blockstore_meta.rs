use {
    crate::shred::{Shred, ShredType},
    bitflags::bitflags,
    serde::{Deserialize, Deserializer, Serialize, Serializer},
    solana_sdk::{
        clock::{Slot, UnixTimestamp},
        hash::Hash,
    },
    std::{
        collections::BTreeSet,
        ops::{Range, RangeBounds},
    },
};

bitflags! {
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
    /// Flags to indicate whether a slot is a descendant of a slot on the main fork
    pub struct ConnectedFlags:u8 {
        // A slot S should be considered to be connected if:
        // 1) S is a rooted slot itself OR
        // 2) S's parent is connected AND S is full (S's complete block present)
        //
        // 1) is a straightfoward case, roots are finalized blocks on the main fork
        // so by definition, they are connected. All roots are connected, but not
        // all connected slots are (or will become) roots.
        //
        // Based on the criteria stated in 2), S is connected iff it has a series
        // of ancestors (that are each connected) that form a chain back to
        // some root slot.
        //
        // A ledger that is updating with a cluster will have either begun at
        // genesis or at at some snapshot slot.
        // - Genesis is obviously a special case, and slot 0's parent is deemed
        //   to be connected in order to kick off the induction
        // - Snapshots are taken at rooted slots, and as such, the snapshot slot
        //   should be marked as connected so that a connected chain can start
        //
        // CONNECTED is explicitly the first bit to ensure backwards compatibility
        // with the boolean field that ConnectedFlags replaced in SlotMeta.
        const CONNECTED        = 0b0000_0001;
        // PARENT_CONNECTED IS INTENTIIONALLY UNUSED FOR NOW
        const PARENT_CONNECTED = 0b1000_0000;
    }
}

impl Default for ConnectedFlags {
    fn default() -> Self {
        ConnectedFlags::empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
/// The Meta column family
pub struct SlotMeta {
    /// The number of slots above the root (the genesis block). The first
    /// slot has slot 0.
    pub slot: Slot,
    /// The total number of consecutive shreds starting from index 0 we have received for this slot.
    /// At the same time, it is also an index of the first missing shred for this slot, while the
    /// slot is incomplete.
    pub consumed: u64,
    /// The index *plus one* of the highest shred received for this slot.  Useful
    /// for checking if the slot has received any shreds yet, and to calculate the
    /// range where there is one or more holes: `(consumed..received)`.
    pub received: u64,
    /// The timestamp of the first time a shred was added for this slot
    pub first_shred_timestamp: u64,
    /// The index of the shred that is flagged as the last shred for this slot.
    /// None until the shred with LAST_SHRED_IN_SLOT flag is received.
    #[serde(with = "serde_compat")]
    pub last_index: Option<u64>,
    /// The slot height of the block this one derives from.
    /// The parent slot of the head of a detached chain of slots is None.
    #[serde(with = "serde_compat")]
    pub parent_slot: Option<Slot>,
    /// The list of slots, each of which contains a block that derives
    /// from this one.
    pub next_slots: Vec<Slot>,
    /// Connected status flags of this slot
    pub connected_flags: ConnectedFlags,
    /// Shreds indices which are marked data complete.  That is, those that have the
    /// [`ShredFlags::DATA_COMPLETE_SHRED`][`crate::shred::ShredFlags::DATA_COMPLETE_SHRED`] set.
    pub completed_data_indexes: BTreeSet<u32>,
}

// Serde implementation of serialize and deserialize for Option<u64>
// where None is represented as u64::MAX; for backward compatibility.
mod serde_compat {
    use super::*;

    pub(super) fn serialize<S>(val: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        val.unwrap_or(u64::MAX).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = u64::deserialize(deserializer)?;
        Ok((val != u64::MAX).then_some(val))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
/// Index recording presence/absence of shreds
pub struct Index {
    pub slot: Slot,
    data: ShredIndex,
    coding: ShredIndex,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ShredIndex {
    /// Map representing presence/absence of shreds
    index: BTreeSet<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
/// Erasure coding information
pub struct ErasureMeta {
    /// Which erasure set in the slot this is
    set_index: u64,
    /// First coding index in the FEC set
    first_coding_index: u64,
    /// Size of shards in this erasure set
    #[serde(rename = "size")]
    __unused_size: usize,
    /// Erasure configuration for this erasure set
    config: ErasureConfig,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ErasureConfig {
    num_data: usize,
    num_coding: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MerkleRootMeta {
    /// The merkle root, `None` for legacy shreds
    merkle_root: Option<Hash>,
    /// The first received shred index
    first_received_shred_index: u32,
    /// The shred type of the first received shred
    first_received_shred_type: ShredType,
}

#[derive(Deserialize, Serialize)]
pub struct DuplicateSlotProof {
    #[serde(with = "serde_bytes")]
    pub shred1: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub shred2: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ErasureMetaStatus {
    CanRecover,
    DataFull,
    StillNeed(usize),
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
pub enum FrozenHashVersioned {
    Current(FrozenHashStatus),
}

impl FrozenHashVersioned {
    pub fn frozen_hash(&self) -> Hash {
        match self {
            FrozenHashVersioned::Current(frozen_hash_status) => frozen_hash_status.frozen_hash,
        }
    }

    pub fn is_duplicate_confirmed(&self) -> bool {
        match self {
            FrozenHashVersioned::Current(frozen_hash_status) => {
                frozen_hash_status.is_duplicate_confirmed
            }
        }
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct FrozenHashStatus {
    pub frozen_hash: Hash,
    pub is_duplicate_confirmed: bool,
}

impl Index {
    pub(crate) fn new(slot: Slot) -> Self {
        Index {
            slot,
            data: ShredIndex::default(),
            coding: ShredIndex::default(),
        }
    }

    pub fn data(&self) -> &ShredIndex {
        &self.data
    }
    pub fn coding(&self) -> &ShredIndex {
        &self.coding
    }

    pub(crate) fn data_mut(&mut self) -> &mut ShredIndex {
        &mut self.data
    }
    pub(crate) fn coding_mut(&mut self) -> &mut ShredIndex {
        &mut self.coding
    }
}

impl ShredIndex {
    pub fn num_shreds(&self) -> usize {
        self.index.len()
    }

    pub(crate) fn range<R>(&self, bounds: R) -> impl Iterator<Item = &u64>
    where
        R: RangeBounds<u64>,
    {
        self.index.range(bounds)
    }

    pub(crate) fn contains(&self, index: u64) -> bool {
        self.index.contains(&index)
    }

    pub(crate) fn insert(&mut self, index: u64) {
        self.index.insert(index);
    }
}

impl SlotMeta {
    pub fn is_full(&self) -> bool {
        // last_index is None when it has no information about how
        // many shreds will fill this slot.
        // Note: A full slot with zero shreds is not possible.
        // Should never happen
        if self
            .last_index
            .map(|ix| self.consumed > ix + 1)
            .unwrap_or_default()
        {
            datapoint_error!(
                "blockstore_error",
                (
                    "error",
                    format!(
                        "Observed a slot meta with consumed: {} > meta.last_index + 1: {:?}",
                        self.consumed,
                        self.last_index.map(|ix| ix + 1),
                    ),
                    String
                )
            );
        }

        Some(self.consumed) == self.last_index.map(|ix| ix + 1)
    }

    /// Returns a boolean indicating whether the meta is connected.
    pub fn is_connected(&self) -> bool {
        self.connected_flags.contains(ConnectedFlags::CONNECTED)
    }

    /// Mark the meta as connected.
    pub fn set_connected(&mut self) {
        assert!(self.is_parent_connected());
        self.connected_flags.set(ConnectedFlags::CONNECTED, true);
    }

    /// Returns a boolean indicating whether the meta's parent is connected.
    pub fn is_parent_connected(&self) -> bool {
        self.connected_flags
            .contains(ConnectedFlags::PARENT_CONNECTED)
    }

    /// Mark the meta's parent as connected.
    /// If the meta is also full, the meta is now connected as well. Return a
    /// boolean indicating whether the meta becamed connected from this call.
    pub fn set_parent_connected(&mut self) -> bool {
        // Already connected so nothing to do, bail early
        if self.is_connected() {
            return false;
        }

        self.connected_flags
            .set(ConnectedFlags::PARENT_CONNECTED, true);

        if self.is_full() {
            self.set_connected();
        }

        self.is_connected()
    }

    /// Dangerous.
    #[cfg(feature = "dev-context-only-utils")]
    pub fn unset_parent(&mut self) {
        self.parent_slot = None;
    }

    pub fn clear_unconfirmed_slot(&mut self) {
        let old = std::mem::replace(self, SlotMeta::new_orphan(self.slot));
        self.next_slots = old.next_slots;
    }

    pub(crate) fn new(slot: Slot, parent_slot: Option<Slot>) -> Self {
        let connected_flags = if slot == 0 {
            // Slot 0 is the start, mark it as having its' parent connected
            // such that slot 0 becoming full will be updated as connected
            ConnectedFlags::PARENT_CONNECTED
        } else {
            ConnectedFlags::default()
        };
        SlotMeta {
            slot,
            parent_slot,
            connected_flags,
            ..SlotMeta::default()
        }
    }

    pub(crate) fn new_orphan(slot: Slot) -> Self {
        Self::new(slot, /*parent_slot:*/ None)
    }
}

impl ErasureMeta {
    pub(crate) fn from_coding_shred(shred: &Shred) -> Option<Self> {
        match shred.shred_type() {
            ShredType::Data => None,
            ShredType::Code => {
                let config = ErasureConfig {
                    num_data: usize::from(shred.num_data_shreds().ok()?),
                    num_coding: usize::from(shred.num_coding_shreds().ok()?),
                };
                let first_coding_index = u64::from(shred.first_coding_index()?);
                let erasure_meta = ErasureMeta {
                    set_index: u64::from(shred.fec_set_index()),
                    config,
                    first_coding_index,
                    __unused_size: 0,
                };
                Some(erasure_meta)
            }
        }
    }

    // Returns true if the erasure fields on the shred
    // are consistent with the erasure-meta.
    pub(crate) fn check_coding_shred(&self, shred: &Shred) -> bool {
        let Some(mut other) = Self::from_coding_shred(shred) else {
            return false;
        };
        other.__unused_size = self.__unused_size;
        self == &other
    }

    /// Returns true if both shreds are coding shreds and have a
    /// consistent erasure config
    pub fn check_erasure_consistency(shred1: &Shred, shred2: &Shred) -> bool {
        let Some(coding_shred) = Self::from_coding_shred(shred1) else {
            return false;
        };
        coding_shred.check_coding_shred(shred2)
    }

    pub(crate) fn config(&self) -> ErasureConfig {
        self.config
    }

    pub(crate) fn data_shreds_indices(&self) -> Range<u64> {
        let num_data = self.config.num_data as u64;
        self.set_index..self.set_index + num_data
    }

    pub(crate) fn coding_shreds_indices(&self) -> Range<u64> {
        let num_coding = self.config.num_coding as u64;
        self.first_coding_index..self.first_coding_index + num_coding
    }

    pub(crate) fn status(&self, index: &Index) -> ErasureMetaStatus {
        use ErasureMetaStatus::*;

        let num_coding = index.coding().range(self.coding_shreds_indices()).count();
        let num_data = index.data().range(self.data_shreds_indices()).count();

        let (data_missing, num_needed) = (
            self.config.num_data.saturating_sub(num_data),
            self.config.num_data.saturating_sub(num_data + num_coding),
        );

        if data_missing == 0 {
            DataFull
        } else if num_needed == 0 {
            CanRecover
        } else {
            StillNeed(num_needed)
        }
    }
}

#[allow(dead_code)]
impl MerkleRootMeta {
    pub(crate) fn from_shred(shred: &Shred) -> Self {
        Self {
            // An error here after the shred has already sigverified
            // can only indicate that the leader is sending
            // legacy or malformed shreds. We should still store
            // `None` for those cases in blockstore, as a later
            // shred that contains a proper merkle root would constitute
            // a valid duplicate shred proof.
            merkle_root: shred.merkle_root().ok(),
            first_received_shred_index: shred.index(),
            first_received_shred_type: shred.shred_type(),
        }
    }

    pub(crate) fn merkle_root(&self) -> Option<Hash> {
        self.merkle_root
    }

    pub(crate) fn first_received_shred_index(&self) -> u32 {
        self.first_received_shred_index
    }

    pub(crate) fn first_received_shred_type(&self) -> ShredType {
        self.first_received_shred_type
    }
}

impl DuplicateSlotProof {
    pub(crate) fn new(shred1: Vec<u8>, shred2: Vec<u8>) -> Self {
        DuplicateSlotProof { shred1, shred2 }
    }
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TransactionStatusIndexMeta {
    pub max_slot: Slot,
    pub frozen: bool,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct AddressSignatureMeta {
    pub writeable: bool,
}

/// Performance information about validator execution during a time slice.
///
/// Older versions should only arise as a result of deserialization of entries stored by a previous
/// version of the validator.  Current version should only produce [`PerfSampleV2`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerfSample {
    V1(PerfSampleV1),
    V2(PerfSampleV2),
}

impl From<PerfSampleV1> for PerfSample {
    fn from(value: PerfSampleV1) -> PerfSample {
        PerfSample::V1(value)
    }
}

impl From<PerfSampleV2> for PerfSample {
    fn from(value: PerfSampleV2) -> PerfSample {
        PerfSample::V2(value)
    }
}

/// Version of [`PerfSample`] used before 1.15.x.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PerfSampleV1 {
    pub num_transactions: u64,
    pub num_slots: u64,
    pub sample_period_secs: u16,
}

/// Version of the [`PerfSample`] introduced in 1.15.x.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PerfSampleV2 {
    // `PerfSampleV1` part
    pub num_transactions: u64,
    pub num_slots: u64,
    pub sample_period_secs: u16,

    // New fields.
    pub num_non_vote_transactions: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProgramCost {
    pub cost: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct OptimisticSlotMetaV0 {
    pub hash: Hash,
    pub timestamp: UnixTimestamp,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
pub enum OptimisticSlotMetaVersioned {
    V0(OptimisticSlotMetaV0),
}

impl OptimisticSlotMetaVersioned {
    pub fn new(hash: Hash, timestamp: UnixTimestamp) -> Self {
        OptimisticSlotMetaVersioned::V0(OptimisticSlotMetaV0 { hash, timestamp })
    }

    pub fn hash(&self) -> Hash {
        match self {
            OptimisticSlotMetaVersioned::V0(meta) => meta.hash,
        }
    }

    pub fn timestamp(&self) -> UnixTimestamp {
        match self {
            OptimisticSlotMetaVersioned::V0(meta) => meta.timestamp,
        }
    }
}
#[cfg(test)]
mod test {
    use {
        super::*,
        rand::{seq::SliceRandom, thread_rng},
    };

    #[test]
    fn test_slot_meta_slot_zero_connected() {
        let meta = SlotMeta::new(0 /* slot */, None /* parent */);
        assert!(meta.is_parent_connected());
        assert!(!meta.is_connected());
    }

    #[test]
    fn test_erasure_meta_status() {
        use ErasureMetaStatus::*;

        let set_index = 0;
        let erasure_config = ErasureConfig {
            num_data: 8,
            num_coding: 16,
        };
        let e_meta = ErasureMeta {
            set_index,
            first_coding_index: set_index,
            config: erasure_config,
            __unused_size: 0,
        };
        let mut rng = thread_rng();
        let mut index = Index::new(0);

        let data_indexes = 0..erasure_config.num_data as u64;
        let coding_indexes = 0..erasure_config.num_coding as u64;

        assert_eq!(e_meta.status(&index), StillNeed(erasure_config.num_data));

        for ix in data_indexes.clone() {
            index.data_mut().insert(ix);
        }

        assert_eq!(e_meta.status(&index), DataFull);

        for ix in coding_indexes.clone() {
            index.coding_mut().insert(ix);
        }

        for &idx in data_indexes
            .clone()
            .collect::<Vec<_>>()
            .choose_multiple(&mut rng, erasure_config.num_data)
        {
            index.data_mut().index.remove(&idx);

            assert_eq!(e_meta.status(&index), CanRecover);
        }

        for ix in data_indexes {
            index.data_mut().insert(ix);
        }

        for &idx in coding_indexes
            .collect::<Vec<_>>()
            .choose_multiple(&mut rng, erasure_config.num_coding)
        {
            index.coding_mut().index.remove(&idx);

            assert_eq!(e_meta.status(&index), DataFull);
        }
    }

    #[test]
    fn test_connected_flags_compatibility() {
        // Define a couple structs with bool and ConnectedFlags to illustrate
        // that that ConnectedFlags can be deserialized into a bool if the
        // PARENT_CONNECTED bit is NOT set
        #[derive(Debug, Deserialize, PartialEq, Serialize)]
        struct WithBool {
            slot: Slot,
            connected: bool,
        }
        #[derive(Debug, Deserialize, PartialEq, Serialize)]
        struct WithFlags {
            slot: Slot,
            connected: ConnectedFlags,
        }

        let slot = 3;
        let mut with_bool = WithBool {
            slot,
            connected: false,
        };
        let mut with_flags = WithFlags {
            slot,
            connected: ConnectedFlags::default(),
        };

        // Confirm that serialized byte arrays are same length
        assert_eq!(
            bincode::serialized_size(&with_bool).unwrap(),
            bincode::serialized_size(&with_flags).unwrap()
        );

        // Confirm that connected=false equivalent to ConnectedFlags::default()
        assert_eq!(
            bincode::serialize(&with_bool).unwrap(),
            bincode::serialize(&with_flags).unwrap()
        );

        // Set connected in WithBool and confirm inequality
        with_bool.connected = true;
        assert_ne!(
            bincode::serialize(&with_bool).unwrap(),
            bincode::serialize(&with_flags).unwrap()
        );

        // Set connected in WithFlags and confirm equality regained
        with_flags.connected.set(ConnectedFlags::CONNECTED, true);
        assert_eq!(
            bincode::serialize(&with_bool).unwrap(),
            bincode::serialize(&with_flags).unwrap()
        );

        // Dserializing WithBool into WithFlags succeeds
        assert_eq!(
            with_flags,
            bincode::deserialize::<WithFlags>(&bincode::serialize(&with_bool).unwrap()).unwrap()
        );

        // Deserializing WithFlags into WithBool succeeds
        assert_eq!(
            with_bool,
            bincode::deserialize::<WithBool>(&bincode::serialize(&with_flags).unwrap()).unwrap()
        );

        // Deserializing WithFlags with extra bit set into WithBool fails
        with_flags
            .connected
            .set(ConnectedFlags::PARENT_CONNECTED, true);
        assert!(
            bincode::deserialize::<WithBool>(&bincode::serialize(&with_flags).unwrap()).is_err()
        );
    }

    #[test]
    fn test_clear_unconfirmed_slot() {
        let mut slot_meta = SlotMeta::new_orphan(5);
        slot_meta.consumed = 5;
        slot_meta.received = 5;
        slot_meta.next_slots = vec![6, 7];
        slot_meta.clear_unconfirmed_slot();

        let mut expected = SlotMeta::new_orphan(5);
        expected.next_slots = vec![6, 7];
        assert_eq!(slot_meta, expected);
    }

    // `PerfSampleV2` should contain `PerfSampleV1` as a prefix, in order for the column to be
    // backward and forward compatible.
    #[test]
    fn perf_sample_v1_is_prefix_of_perf_sample_v2() {
        let v2 = PerfSampleV2 {
            num_transactions: 4190143848,
            num_slots: 3607325588,
            sample_period_secs: 31263,
            num_non_vote_transactions: 4056116066,
        };

        let v2_bytes = bincode::serialize(&v2).expect("`PerfSampleV2` can be serialized");

        let actual: PerfSampleV1 = bincode::deserialize(&v2_bytes)
            .expect("Bytes encoded as `PerfSampleV2` can be decoded as `PerfSampleV1`");
        let expected = PerfSampleV1 {
            num_transactions: v2.num_transactions,
            num_slots: v2.num_slots,
            sample_period_secs: v2.sample_period_secs,
        };

        assert_eq!(actual, expected);
    }
}
