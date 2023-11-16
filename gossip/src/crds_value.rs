use {
    crate::{
        cluster_info::MAX_ACCOUNTS_HASHES,
        contact_info::ContactInfo,
        deprecated,
        duplicate_shred::{DuplicateShred, DuplicateShredIndex, MAX_DUPLICATE_SHREDS},
        epoch_slots::EpochSlots,
        legacy_contact_info::LegacyContactInfo,
    },
    bincode::{serialize, serialized_size},
    bv::BitVec,
    itertools::Itertools,
    rand::{CryptoRng, Rng},
    serde::de::{Deserialize, Deserializer},
    solana_sdk::{
        clock::Slot,
        hash::Hash,
        pubkey::{self, Pubkey},
        sanitize::{Sanitize, SanitizeError},
        serde_varint,
        signature::{Keypair, Signable, Signature, Signer},
        timing::timestamp,
        transaction::Transaction,
    },
    solana_vote::vote_parser,
    std::{
        borrow::{Borrow, Cow},
        cmp::Ordering,
        collections::{hash_map::Entry, BTreeSet, HashMap},
        fmt,
    },
    thiserror::Error,
};

pub const MAX_WALLCLOCK: u64 = 1_000_000_000_000_000;
pub const MAX_SLOT: u64 = 1_000_000_000_000_000;

pub type VoteIndex = u8;
// TODO: Remove this in favor of vote_state::MAX_LOCKOUT_HISTORY once
// the fleet is updated to the new ClusterInfo::push_vote code.
pub const MAX_VOTES: VoteIndex = 32;

pub type EpochSlotsIndex = u8;
pub const MAX_EPOCH_SLOTS: EpochSlotsIndex = 255;

/// CrdsValue that is replicated across the cluster
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct CrdsValue {
    pub signature: Signature,
    pub data: CrdsData,
}

impl Sanitize for CrdsValue {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        self.signature.sanitize()?;
        self.data.sanitize()
    }
}

impl Signable for CrdsValue {
    fn pubkey(&self) -> Pubkey {
        self.pubkey()
    }

    fn signable_data(&self) -> Cow<[u8]> {
        Cow::Owned(serialize(&self.data).expect("failed to serialize CrdsData"))
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature
    }

    fn verify(&self) -> bool {
        self.get_signature()
            .verify(self.pubkey().as_ref(), self.signable_data().borrow())
    }
}

/// CrdsData that defines the different types of items CrdsValues can hold
/// * Merge Strategy - Latest wallclock is picked
/// * LowestSlot index is deprecated
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample, AbiEnumVisitor)]
pub enum CrdsData {
    LegacyContactInfo(LegacyContactInfo),
    Vote(VoteIndex, Vote),
    LowestSlot(/*DEPRECATED:*/ u8, LowestSlot),
    LegacySnapshotHashes(LegacySnapshotHashes), // Deprecated
    AccountsHashes(AccountsHashes),
    EpochSlots(EpochSlotsIndex, EpochSlots),
    LegacyVersion(LegacyVersion),
    Version(Version),
    NodeInstance(NodeInstance),
    DuplicateShred(DuplicateShredIndex, DuplicateShred),
    SnapshotHashes(SnapshotHashes),
    ContactInfo(ContactInfo),
    RestartLastVotedForkSlots(RestartLastVotedForkSlots),
}

impl Sanitize for CrdsData {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        match self {
            CrdsData::LegacyContactInfo(val) => val.sanitize(),
            CrdsData::Vote(ix, val) => {
                if *ix >= MAX_VOTES {
                    return Err(SanitizeError::ValueOutOfBounds);
                }
                val.sanitize()
            }
            CrdsData::LowestSlot(ix, val) => {
                if *ix as usize >= 1 {
                    return Err(SanitizeError::ValueOutOfBounds);
                }
                val.sanitize()
            }
            CrdsData::LegacySnapshotHashes(val) => val.sanitize(),
            CrdsData::AccountsHashes(val) => val.sanitize(),
            CrdsData::EpochSlots(ix, val) => {
                if *ix as usize >= MAX_EPOCH_SLOTS as usize {
                    return Err(SanitizeError::ValueOutOfBounds);
                }
                val.sanitize()
            }
            CrdsData::LegacyVersion(version) => version.sanitize(),
            CrdsData::Version(version) => version.sanitize(),
            CrdsData::NodeInstance(node) => node.sanitize(),
            CrdsData::DuplicateShred(ix, shred) => {
                if *ix >= MAX_DUPLICATE_SHREDS {
                    Err(SanitizeError::ValueOutOfBounds)
                } else {
                    shred.sanitize()
                }
            }
            CrdsData::SnapshotHashes(val) => val.sanitize(),
            CrdsData::ContactInfo(node) => node.sanitize(),
            CrdsData::RestartLastVotedForkSlots(slots) => slots.sanitize(),
        }
    }
}

/// Random timestamp for tests and benchmarks.
pub(crate) fn new_rand_timestamp<R: Rng>(rng: &mut R) -> u64 {
    const DELAY: u64 = 10 * 60 * 1000; // 10 minutes
    timestamp() - DELAY + rng.gen_range(0..2 * DELAY)
}

impl CrdsData {
    /// New random CrdsData for tests and benchmarks.
    fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> CrdsData {
        let kind = rng.gen_range(0..8);
        // TODO: Implement other kinds of CrdsData here.
        // TODO: Assign ranges to each arm proportional to their frequency in
        // the mainnet crds table.
        match kind {
            0 => CrdsData::LegacyContactInfo(LegacyContactInfo::new_rand(rng, pubkey)),
            // Index for LowestSlot is deprecated and should be zero.
            1 => CrdsData::LowestSlot(0, LowestSlot::new_rand(rng, pubkey)),
            2 => CrdsData::LegacySnapshotHashes(LegacySnapshotHashes::new_rand(rng, pubkey)),
            3 => CrdsData::AccountsHashes(AccountsHashes::new_rand(rng, pubkey)),
            4 => CrdsData::Version(Version::new_rand(rng, pubkey)),
            5 => CrdsData::Vote(rng.gen_range(0..MAX_VOTES), Vote::new_rand(rng, pubkey)),
            6 => CrdsData::RestartLastVotedForkSlots(RestartLastVotedForkSlots::new_rand(
                rng, pubkey,
            )),
            _ => CrdsData::EpochSlots(
                rng.gen_range(0..MAX_EPOCH_SLOTS),
                EpochSlots::new_rand(rng, pubkey),
            ),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct AccountsHashes {
    pub from: Pubkey,
    pub hashes: Vec<(Slot, Hash)>,
    pub wallclock: u64,
}

impl Sanitize for AccountsHashes {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        for (slot, _) in &self.hashes {
            if *slot >= MAX_SLOT {
                return Err(SanitizeError::ValueOutOfBounds);
            }
        }
        self.from.sanitize()
    }
}

impl AccountsHashes {
    pub fn new(from: Pubkey, hashes: Vec<(Slot, Hash)>) -> Self {
        Self {
            from,
            hashes,
            wallclock: timestamp(),
        }
    }

    /// New random AccountsHashes for tests and benchmarks.
    pub(crate) fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        let num_hashes = rng.gen_range(0..MAX_ACCOUNTS_HASHES) + 1;
        let hashes = std::iter::repeat_with(|| {
            let slot = 47825632 + rng.gen_range(0..512);
            let hash = Hash::new_unique();
            (slot, hash)
        })
        .take(num_hashes)
        .collect();
        Self {
            from: pubkey.unwrap_or_else(pubkey::new_rand),
            hashes,
            wallclock: new_rand_timestamp(rng),
        }
    }
}

type LegacySnapshotHashes = AccountsHashes;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct SnapshotHashes {
    pub from: Pubkey,
    pub full: (Slot, Hash),
    pub incremental: Vec<(Slot, Hash)>,
    pub wallclock: u64,
}

impl Sanitize for SnapshotHashes {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        if self.full.0 >= MAX_SLOT {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        for (slot, _) in &self.incremental {
            if *slot >= MAX_SLOT {
                return Err(SanitizeError::ValueOutOfBounds);
            }
            if self.full.0 >= *slot {
                return Err(SanitizeError::InvalidValue);
            }
        }
        self.from.sanitize()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct LowestSlot {
    pub from: Pubkey,
    root: Slot, //deprecated
    pub lowest: Slot,
    slots: BTreeSet<Slot>,                        //deprecated
    stash: Vec<deprecated::EpochIncompleteSlots>, //deprecated
    pub wallclock: u64,
}

impl LowestSlot {
    pub fn new(from: Pubkey, lowest: Slot, wallclock: u64) -> Self {
        Self {
            from,
            root: 0,
            lowest,
            slots: BTreeSet::new(),
            stash: vec![],
            wallclock,
        }
    }

    /// New random LowestSlot for tests and benchmarks.
    fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        Self {
            from: pubkey.unwrap_or_else(pubkey::new_rand),
            root: rng.gen(),
            lowest: rng.gen(),
            slots: BTreeSet::default(),
            stash: Vec::default(),
            wallclock: new_rand_timestamp(rng),
        }
    }
}

impl Sanitize for LowestSlot {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        if self.lowest >= MAX_SLOT {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        if self.root != 0 {
            return Err(SanitizeError::InvalidValue);
        }
        if !self.slots.is_empty() {
            return Err(SanitizeError::InvalidValue);
        }
        if !self.stash.is_empty() {
            return Err(SanitizeError::InvalidValue);
        }
        self.from.sanitize()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, AbiExample, Serialize)]
pub struct Vote {
    pub(crate) from: Pubkey,
    transaction: Transaction,
    pub(crate) wallclock: u64,
    #[serde(skip_serializing)]
    slot: Option<Slot>,
}

impl Sanitize for Vote {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.from.sanitize()?;
        self.transaction.sanitize()
    }
}

impl Vote {
    // Returns None if cannot parse transaction into a vote.
    pub fn new(from: Pubkey, transaction: Transaction, wallclock: u64) -> Option<Self> {
        vote_parser::parse_vote_transaction(&transaction).map(|(_, vote, ..)| Self {
            from,
            transaction,
            wallclock,
            slot: vote.last_voted_slot(),
        })
    }

    /// New random Vote for tests and benchmarks.
    fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        Self {
            from: pubkey.unwrap_or_else(pubkey::new_rand),
            transaction: Transaction::default(),
            wallclock: new_rand_timestamp(rng),
            slot: None,
        }
    }

    pub(crate) fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    pub(crate) fn slot(&self) -> Option<Slot> {
        self.slot
    }
}

impl<'de> Deserialize<'de> for Vote {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Vote {
            from: Pubkey,
            transaction: Transaction,
            wallclock: u64,
        }
        let vote = Vote::deserialize(deserializer)?;
        vote.transaction
            .sanitize()
            .map_err(serde::de::Error::custom)?;
        Self::new(vote.from, vote.transaction, vote.wallclock)
            .ok_or_else(|| serde::de::Error::custom("invalid vote tx"))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct LegacyVersion {
    pub from: Pubkey,
    pub wallclock: u64,
    pub version: solana_version::LegacyVersion1,
}

impl Sanitize for LegacyVersion {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.from.sanitize()?;
        self.version.sanitize()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct Version {
    pub from: Pubkey,
    pub wallclock: u64,
    pub version: solana_version::LegacyVersion2,
}

impl Sanitize for Version {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.from.sanitize()?;
        self.version.sanitize()
    }
}

impl Version {
    pub fn new(from: Pubkey) -> Self {
        Self {
            from,
            wallclock: timestamp(),
            version: solana_version::LegacyVersion2::default(),
        }
    }

    /// New random Version for tests and benchmarks.
    fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        Self {
            from: pubkey.unwrap_or_else(pubkey::new_rand),
            wallclock: new_rand_timestamp(rng),
            version: solana_version::LegacyVersion2 {
                major: rng.gen(),
                minor: rng.gen(),
                patch: rng.gen(),
                commit: Some(rng.gen()),
                feature_set: rng.gen(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, AbiExample, Deserialize, Serialize)]
pub struct NodeInstance {
    from: Pubkey,
    wallclock: u64,
    timestamp: u64, // Timestamp when the instance was created.
    token: u64,     // Randomly generated value at node instantiation.
}

impl NodeInstance {
    pub fn new<R>(rng: &mut R, from: Pubkey, now: u64) -> Self
    where
        R: Rng + CryptoRng,
    {
        Self {
            from,
            wallclock: now,
            timestamp: now,
            token: rng.gen(),
        }
    }

    // Clones the value with an updated wallclock.
    pub(crate) fn with_wallclock(&self, wallclock: u64) -> Self {
        Self { wallclock, ..*self }
    }

    // Returns true if the crds-value is a duplicate instance of this node,
    // with a more recent timestamp.
    // The older instance is considered the duplicate instance. If a staked
    // node is restarted it will receive its old instance value from gossip.
    // Considering the new instance as the duplicate would prevent the node
    // from restarting.
    pub(crate) fn check_duplicate(&self, other: &CrdsValue) -> bool {
        match &other.data {
            CrdsData::NodeInstance(other) => {
                self.token != other.token
                    && self.timestamp <= other.timestamp
                    && self.from == other.from
            }
            _ => false,
        }
    }

    // Returns None if tokens are the same or other is not a node-instance from
    // the same owner. Otherwise returns true if self has more recent timestamp
    // than other, and so overrides it.
    pub(crate) fn overrides(&self, other: &CrdsValue) -> Option<bool> {
        let CrdsData::NodeInstance(other) = &other.data else {
            return None;
        };
        if self.token == other.token || self.from != other.from {
            return None;
        }
        match self.timestamp.cmp(&other.timestamp) {
            Ordering::Less => Some(false),
            Ordering::Greater => Some(true),
            // Ties should be broken in a deterministic way across the cluster,
            // so that nodes propagate the same value through gossip.
            Ordering::Equal => Some(other.token < self.token),
        }
    }
}

impl Sanitize for NodeInstance {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.from.sanitize()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample, AbiEnumVisitor)]
enum SlotsOffsets {
    RunLengthEncoding(RunLengthEncoding),
    RawOffsets(RawOffsets),
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, AbiExample)]
struct U16(#[serde(with = "serde_varint")] u16);

// The vector always starts with 1. Encode number of 1's and 0's consecutively.
// For example, 110000111 is [2, 4, 3].
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, AbiExample)]
struct RunLengthEncoding(Vec<U16>);

impl RunLengthEncoding {
    fn new(bits: &BitVec<u8>) -> Self {
        let encoded = (0..bits.len())
            .map(|i| bits.get(i))
            .dedup_with_count()
            .map_while(|(count, _)| u16::try_from(count).ok())
            .scan(0, |current_bytes, count| {
                *current_bytes += ((u16::BITS - count.leading_zeros() + 6) / 7).max(1) as usize;
                (*current_bytes <= RestartLastVotedForkSlots::MAX_BYTES).then_some(U16(count))
            })
            .collect();
        Self(encoded)
    }

    fn num_encoded_slots(&self) -> usize {
        self.0
            .iter()
            .map(|x| usize::try_from(x.0).unwrap())
            .sum::<usize>()
    }

    fn to_slots(&self, last_slot: Slot, min_slot: Slot) -> Vec<Slot> {
        let mut slots: Vec<Slot> = self
            .0
            .iter()
            .map_while(|bit_count| usize::try_from(bit_count.0).ok())
            .zip([1, 0].iter().cycle())
            .flat_map(|(bit_count, bit)| std::iter::repeat(bit).take(bit_count))
            .enumerate()
            .filter(|(_, bit)| **bit == 1)
            .map_while(|(offset, _)| {
                let offset = Slot::try_from(offset).ok()?;
                last_slot.checked_sub(offset)
            })
            .take(RestartLastVotedForkSlots::MAX_SLOTS)
            .take_while(|slot| *slot >= min_slot)
            .collect();
        slots.reverse();
        slots
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, AbiExample)]
struct RawOffsets(BitVec<u8>);

impl RawOffsets {
    fn new(mut bits: BitVec<u8>) -> Self {
        bits.truncate(RestartLastVotedForkSlots::MAX_BYTES as u64 * 8);
        bits.shrink_to_fit();
        Self(bits)
    }

    fn to_slots(&self, last_slot: Slot, min_slot: Slot) -> Vec<Slot> {
        let mut slots: Vec<Slot> = (0..self.0.len())
            .filter(|index| self.0.get(*index))
            .map_while(|offset| last_slot.checked_sub(offset))
            .take_while(|slot| *slot >= min_slot)
            .collect();
        slots.reverse();
        slots
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, AbiExample, Debug)]
pub struct RestartLastVotedForkSlots {
    pub from: Pubkey,
    pub wallclock: u64,
    offsets: SlotsOffsets,
    pub last_voted_slot: Slot,
    pub last_voted_hash: Hash,
    pub shred_version: u16,
}

impl Sanitize for RestartLastVotedForkSlots {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        self.last_voted_hash.sanitize()
    }
}

#[derive(Debug, Error)]
pub enum RestartLastVotedForkSlotsError {
    #[error("Last voted fork cannot be empty")]
    LastVotedForkEmpty,
}

impl RestartLastVotedForkSlots {
    // This number is MAX_CRDS_OBJECT_SIZE - empty serialized RestartLastVotedForkSlots.
    const MAX_BYTES: usize = 824;

    // Per design doc, we should start wen_restart within 7 hours.
    pub const MAX_SLOTS: usize = u16::MAX as usize;

    pub fn new(
        from: Pubkey,
        now: u64,
        last_voted_fork: &[Slot],
        last_voted_hash: Hash,
        shred_version: u16,
    ) -> Result<Self, RestartLastVotedForkSlotsError> {
        let Some((&first_voted_slot, &last_voted_slot)) =
            last_voted_fork.iter().minmax().into_option()
        else {
            return Err(RestartLastVotedForkSlotsError::LastVotedForkEmpty);
        };
        let max_size = last_voted_slot.saturating_sub(first_voted_slot) + 1;
        let mut uncompressed_bitvec = BitVec::new_fill(false, max_size);
        for slot in last_voted_fork {
            uncompressed_bitvec.set(last_voted_slot - *slot, true);
        }
        let run_length_encoding = RunLengthEncoding::new(&uncompressed_bitvec);
        let offsets =
            if run_length_encoding.num_encoded_slots() > RestartLastVotedForkSlots::MAX_BYTES * 8 {
                SlotsOffsets::RunLengthEncoding(run_length_encoding)
            } else {
                SlotsOffsets::RawOffsets(RawOffsets::new(uncompressed_bitvec))
            };
        Ok(Self {
            from,
            wallclock: now,
            offsets,
            last_voted_slot,
            last_voted_hash,
            shred_version,
        })
    }

    /// New random Version for tests and benchmarks.
    pub fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> Self {
        let pubkey = pubkey.unwrap_or_else(solana_sdk::pubkey::new_rand);
        let num_slots = rng.gen_range(2..20);
        let slots = std::iter::repeat_with(|| 47825632 + rng.gen_range(0..512))
            .take(num_slots)
            .collect::<Vec<Slot>>();
        RestartLastVotedForkSlots::new(
            pubkey,
            new_rand_timestamp(rng),
            &slots,
            Hash::new_unique(),
            1,
        )
        .unwrap()
    }

    pub fn to_slots(&self, min_slot: Slot) -> Vec<Slot> {
        match &self.offsets {
            SlotsOffsets::RunLengthEncoding(run_length_encoding) => {
                run_length_encoding.to_slots(self.last_voted_slot, min_slot)
            }
            SlotsOffsets::RawOffsets(raw_offsets) => {
                raw_offsets.to_slots(self.last_voted_slot, min_slot)
            }
        }
    }
}

/// Type of the replicated value
/// These are labels for values in a record that is associated with `Pubkey`
#[derive(PartialEq, Hash, Eq, Clone, Debug)]
pub enum CrdsValueLabel {
    LegacyContactInfo(Pubkey),
    Vote(VoteIndex, Pubkey),
    LowestSlot(Pubkey),
    LegacySnapshotHashes(Pubkey),
    EpochSlots(EpochSlotsIndex, Pubkey),
    AccountsHashes(Pubkey),
    LegacyVersion(Pubkey),
    Version(Pubkey),
    NodeInstance(Pubkey),
    DuplicateShred(DuplicateShredIndex, Pubkey),
    SnapshotHashes(Pubkey),
    ContactInfo(Pubkey),
    RestartLastVotedForkSlots(Pubkey),
}

impl fmt::Display for CrdsValueLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrdsValueLabel::LegacyContactInfo(_) => {
                write!(f, "LegacyContactInfo({})", self.pubkey())
            }
            CrdsValueLabel::Vote(ix, _) => write!(f, "Vote({}, {})", ix, self.pubkey()),
            CrdsValueLabel::LowestSlot(_) => write!(f, "LowestSlot({})", self.pubkey()),
            CrdsValueLabel::LegacySnapshotHashes(_) => {
                write!(f, "LegacySnapshotHashes({})", self.pubkey())
            }
            CrdsValueLabel::EpochSlots(ix, _) => write!(f, "EpochSlots({}, {})", ix, self.pubkey()),
            CrdsValueLabel::AccountsHashes(_) => write!(f, "AccountsHashes({})", self.pubkey()),
            CrdsValueLabel::LegacyVersion(_) => write!(f, "LegacyVersion({})", self.pubkey()),
            CrdsValueLabel::Version(_) => write!(f, "Version({})", self.pubkey()),
            CrdsValueLabel::NodeInstance(pk) => write!(f, "NodeInstance({pk})"),
            CrdsValueLabel::DuplicateShred(ix, pk) => write!(f, "DuplicateShred({ix}, {pk})"),
            CrdsValueLabel::SnapshotHashes(_) => {
                write!(f, "SnapshotHashes({})", self.pubkey())
            }
            CrdsValueLabel::ContactInfo(_) => write!(f, "ContactInfo({})", self.pubkey()),
            CrdsValueLabel::RestartLastVotedForkSlots(_) => {
                write!(f, "RestartLastVotedForkSlots({})", self.pubkey())
            }
        }
    }
}

impl CrdsValueLabel {
    pub fn pubkey(&self) -> Pubkey {
        match self {
            CrdsValueLabel::LegacyContactInfo(p) => *p,
            CrdsValueLabel::Vote(_, p) => *p,
            CrdsValueLabel::LowestSlot(p) => *p,
            CrdsValueLabel::LegacySnapshotHashes(p) => *p,
            CrdsValueLabel::EpochSlots(_, p) => *p,
            CrdsValueLabel::AccountsHashes(p) => *p,
            CrdsValueLabel::LegacyVersion(p) => *p,
            CrdsValueLabel::Version(p) => *p,
            CrdsValueLabel::NodeInstance(p) => *p,
            CrdsValueLabel::DuplicateShred(_, p) => *p,
            CrdsValueLabel::SnapshotHashes(p) => *p,
            CrdsValueLabel::ContactInfo(pubkey) => *pubkey,
            CrdsValueLabel::RestartLastVotedForkSlots(p) => *p,
        }
    }
}

impl CrdsValue {
    pub fn new_unsigned(data: CrdsData) -> Self {
        Self {
            signature: Signature::default(),
            data,
        }
    }

    pub fn new_signed(data: CrdsData, keypair: &Keypair) -> Self {
        let mut value = Self::new_unsigned(data);
        value.sign(keypair);
        value
    }

    /// New random CrdsValue for tests and benchmarks.
    pub fn new_rand<R: Rng>(rng: &mut R, keypair: Option<&Keypair>) -> CrdsValue {
        match keypair {
            None => {
                let keypair = Keypair::new();
                let data = CrdsData::new_rand(rng, Some(keypair.pubkey()));
                Self::new_signed(data, &keypair)
            }
            Some(keypair) => {
                let data = CrdsData::new_rand(rng, Some(keypair.pubkey()));
                Self::new_signed(data, keypair)
            }
        }
    }

    /// Totally unsecure unverifiable wallclock of the node that generated this message
    /// Latest wallclock is always picked.
    /// This is used to time out push messages.
    pub fn wallclock(&self) -> u64 {
        match &self.data {
            CrdsData::LegacyContactInfo(contact_info) => contact_info.wallclock(),
            CrdsData::Vote(_, vote) => vote.wallclock,
            CrdsData::LowestSlot(_, obj) => obj.wallclock,
            CrdsData::LegacySnapshotHashes(hash) => hash.wallclock,
            CrdsData::AccountsHashes(hash) => hash.wallclock,
            CrdsData::EpochSlots(_, p) => p.wallclock,
            CrdsData::LegacyVersion(version) => version.wallclock,
            CrdsData::Version(version) => version.wallclock,
            CrdsData::NodeInstance(node) => node.wallclock,
            CrdsData::DuplicateShred(_, shred) => shred.wallclock,
            CrdsData::SnapshotHashes(hash) => hash.wallclock,
            CrdsData::ContactInfo(node) => node.wallclock(),
            CrdsData::RestartLastVotedForkSlots(slots) => slots.wallclock,
        }
    }
    pub fn pubkey(&self) -> Pubkey {
        match &self.data {
            CrdsData::LegacyContactInfo(contact_info) => *contact_info.pubkey(),
            CrdsData::Vote(_, vote) => vote.from,
            CrdsData::LowestSlot(_, slots) => slots.from,
            CrdsData::LegacySnapshotHashes(hash) => hash.from,
            CrdsData::AccountsHashes(hash) => hash.from,
            CrdsData::EpochSlots(_, p) => p.from,
            CrdsData::LegacyVersion(version) => version.from,
            CrdsData::Version(version) => version.from,
            CrdsData::NodeInstance(node) => node.from,
            CrdsData::DuplicateShred(_, shred) => shred.from,
            CrdsData::SnapshotHashes(hash) => hash.from,
            CrdsData::ContactInfo(node) => *node.pubkey(),
            CrdsData::RestartLastVotedForkSlots(slots) => slots.from,
        }
    }
    pub fn label(&self) -> CrdsValueLabel {
        match &self.data {
            CrdsData::LegacyContactInfo(_) => CrdsValueLabel::LegacyContactInfo(self.pubkey()),
            CrdsData::Vote(ix, _) => CrdsValueLabel::Vote(*ix, self.pubkey()),
            CrdsData::LowestSlot(_, _) => CrdsValueLabel::LowestSlot(self.pubkey()),
            CrdsData::LegacySnapshotHashes(_) => {
                CrdsValueLabel::LegacySnapshotHashes(self.pubkey())
            }
            CrdsData::AccountsHashes(_) => CrdsValueLabel::AccountsHashes(self.pubkey()),
            CrdsData::EpochSlots(ix, _) => CrdsValueLabel::EpochSlots(*ix, self.pubkey()),
            CrdsData::LegacyVersion(_) => CrdsValueLabel::LegacyVersion(self.pubkey()),
            CrdsData::Version(_) => CrdsValueLabel::Version(self.pubkey()),
            CrdsData::NodeInstance(node) => CrdsValueLabel::NodeInstance(node.from),
            CrdsData::DuplicateShred(ix, shred) => CrdsValueLabel::DuplicateShred(*ix, shred.from),
            CrdsData::SnapshotHashes(_) => CrdsValueLabel::SnapshotHashes(self.pubkey()),
            CrdsData::ContactInfo(node) => CrdsValueLabel::ContactInfo(*node.pubkey()),
            CrdsData::RestartLastVotedForkSlots(_) => {
                CrdsValueLabel::RestartLastVotedForkSlots(self.pubkey())
            }
        }
    }
    pub fn contact_info(&self) -> Option<&LegacyContactInfo> {
        match &self.data {
            CrdsData::LegacyContactInfo(contact_info) => Some(contact_info),
            _ => None,
        }
    }

    pub(crate) fn accounts_hash(&self) -> Option<&AccountsHashes> {
        match &self.data {
            CrdsData::AccountsHashes(slots) => Some(slots),
            _ => None,
        }
    }

    pub(crate) fn epoch_slots(&self) -> Option<&EpochSlots> {
        match &self.data {
            CrdsData::EpochSlots(_, slots) => Some(slots),
            _ => None,
        }
    }

    /// Returns the size (in bytes) of a CrdsValue
    pub fn size(&self) -> u64 {
        serialized_size(&self).expect("unable to serialize contact info")
    }

    /// Returns true if, regardless of prunes, this crds-value
    /// should be pushed to the receiving node.
    pub(crate) fn should_force_push(&self, peer: &Pubkey) -> bool {
        match &self.data {
            CrdsData::NodeInstance(node) => node.from == *peer,
            _ => false,
        }
    }
}

/// Filters out an iterator of crds values, returning
/// the unique ones with the most recent wallclock.
pub(crate) fn filter_current<'a, I>(values: I) -> impl Iterator<Item = &'a CrdsValue>
where
    I: IntoIterator<Item = &'a CrdsValue>,
{
    let mut out = HashMap::new();
    for value in values {
        match out.entry(value.label()) {
            Entry::Vacant(entry) => {
                entry.insert((value, value.wallclock()));
            }
            Entry::Occupied(mut entry) => {
                let value_wallclock = value.wallclock();
                let (_, entry_wallclock) = entry.get();
                if *entry_wallclock < value_wallclock {
                    entry.insert((value, value_wallclock));
                }
            }
        }
    }
    out.into_iter().map(|(_, (v, _))| v)
}

pub(crate) fn sanitize_wallclock(wallclock: u64) -> Result<(), SanitizeError> {
    if wallclock >= MAX_WALLCLOCK {
        Err(SanitizeError::ValueOutOfBounds)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::cluster_info::MAX_CRDS_OBJECT_SIZE,
        bincode::{deserialize, Options},
        rand::SeedableRng,
        rand_chacha::ChaChaRng,
        solana_perf::test_tx::new_test_vote_tx,
        solana_sdk::{
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_vote_program::{vote_instruction, vote_state},
        std::{cmp::Ordering, iter::repeat_with},
    };

    #[test]
    fn test_keys_and_values() {
        let mut rng = rand::thread_rng();
        let v = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(LegacyContactInfo::default()));
        assert_eq!(v.wallclock(), 0);
        let key = *v.contact_info().unwrap().pubkey();
        assert_eq!(v.label(), CrdsValueLabel::LegacyContactInfo(key));

        let v = Vote::new(Pubkey::default(), new_test_vote_tx(&mut rng), 0).unwrap();
        let v = CrdsValue::new_unsigned(CrdsData::Vote(0, v));
        assert_eq!(v.wallclock(), 0);
        let key = match &v.data {
            CrdsData::Vote(_, vote) => vote.from,
            _ => panic!(),
        };
        assert_eq!(v.label(), CrdsValueLabel::Vote(0, key));

        let v = CrdsValue::new_unsigned(CrdsData::LowestSlot(
            0,
            LowestSlot::new(Pubkey::default(), 0, 0),
        ));
        assert_eq!(v.wallclock(), 0);
        let key = match &v.data {
            CrdsData::LowestSlot(_, data) => data.from,
            _ => panic!(),
        };
        assert_eq!(v.label(), CrdsValueLabel::LowestSlot(key));
    }

    #[test]
    fn test_lowest_slot_sanitize() {
        let ls = LowestSlot::new(Pubkey::default(), 0, 0);
        let v = CrdsValue::new_unsigned(CrdsData::LowestSlot(0, ls.clone()));
        assert_eq!(v.sanitize(), Ok(()));

        let mut o = ls.clone();
        o.root = 1;
        let v = CrdsValue::new_unsigned(CrdsData::LowestSlot(0, o));
        assert_eq!(v.sanitize(), Err(SanitizeError::InvalidValue));

        let o = ls.clone();
        let v = CrdsValue::new_unsigned(CrdsData::LowestSlot(1, o));
        assert_eq!(v.sanitize(), Err(SanitizeError::ValueOutOfBounds));

        let mut o = ls.clone();
        o.slots.insert(1);
        let v = CrdsValue::new_unsigned(CrdsData::LowestSlot(0, o));
        assert_eq!(v.sanitize(), Err(SanitizeError::InvalidValue));

        let mut o = ls;
        o.stash.push(deprecated::EpochIncompleteSlots::default());
        let v = CrdsValue::new_unsigned(CrdsData::LowestSlot(0, o));
        assert_eq!(v.sanitize(), Err(SanitizeError::InvalidValue));
    }

    #[test]
    fn test_signature() {
        let mut rng = rand::thread_rng();
        let keypair = Keypair::new();
        let wrong_keypair = Keypair::new();
        let mut v = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(
            LegacyContactInfo::new_localhost(&keypair.pubkey(), timestamp()),
        ));
        verify_signatures(&mut v, &keypair, &wrong_keypair);
        let v = Vote::new(keypair.pubkey(), new_test_vote_tx(&mut rng), timestamp()).unwrap();
        let mut v = CrdsValue::new_unsigned(CrdsData::Vote(0, v));
        verify_signatures(&mut v, &keypair, &wrong_keypair);
        v = CrdsValue::new_unsigned(CrdsData::LowestSlot(
            0,
            LowestSlot::new(keypair.pubkey(), 0, timestamp()),
        ));
        verify_signatures(&mut v, &keypair, &wrong_keypair);
    }

    #[test]
    fn test_max_vote_index() {
        let mut rng = rand::thread_rng();
        let keypair = Keypair::new();
        let vote = Vote::new(keypair.pubkey(), new_test_vote_tx(&mut rng), timestamp()).unwrap();
        let vote = CrdsValue::new_signed(CrdsData::Vote(MAX_VOTES, vote), &keypair);
        assert!(vote.sanitize().is_err());
    }

    #[test]
    fn test_vote_round_trip() {
        let mut rng = rand::thread_rng();
        let vote = vote_state::Vote::new(
            vec![1, 3, 7], // slots
            Hash::new_unique(),
        );
        let ix = vote_instruction::vote(
            &Pubkey::new_unique(), // vote_pubkey
            &Pubkey::new_unique(), // authorized_voter_pubkey
            vote,
        );
        let tx = Transaction::new_with_payer(
            &[ix],                       // instructions
            Some(&Pubkey::new_unique()), // payer
        );
        let vote = Vote::new(
            Pubkey::new_unique(), // from
            tx,
            rng.gen(), // wallclock
        )
        .unwrap();
        assert_eq!(vote.slot, Some(7));
        let bytes = bincode::serialize(&vote).unwrap();
        let other = bincode::deserialize(&bytes[..]).unwrap();
        assert_eq!(vote, other);
        assert_eq!(other.slot, Some(7));
        let bytes = bincode::options().serialize(&vote).unwrap();
        let other = bincode::options().deserialize(&bytes[..]).unwrap();
        assert_eq!(vote, other);
        assert_eq!(other.slot, Some(7));
    }

    #[test]
    fn test_max_epoch_slots_index() {
        let keypair = Keypair::new();
        let item = CrdsValue::new_signed(
            CrdsData::EpochSlots(
                MAX_EPOCH_SLOTS,
                EpochSlots::new(keypair.pubkey(), timestamp()),
            ),
            &keypair,
        );
        assert_eq!(item.sanitize(), Err(SanitizeError::ValueOutOfBounds));
    }

    fn serialize_deserialize_value(value: &mut CrdsValue, keypair: &Keypair) {
        let num_tries = 10;
        value.sign(keypair);
        let original_signature = value.get_signature();
        for _ in 0..num_tries {
            let serialized_value = serialize(value).unwrap();
            let deserialized_value: CrdsValue = deserialize(&serialized_value).unwrap();

            // Signatures shouldn't change
            let deserialized_signature = deserialized_value.get_signature();
            assert_eq!(original_signature, deserialized_signature);

            // After deserializing, check that the signature is still the same
            assert!(deserialized_value.verify());
        }
    }

    fn verify_signatures(
        value: &mut CrdsValue,
        correct_keypair: &Keypair,
        wrong_keypair: &Keypair,
    ) {
        assert!(!value.verify());
        value.sign(correct_keypair);
        assert!(value.verify());
        value.sign(wrong_keypair);
        assert!(!value.verify());
        serialize_deserialize_value(value, correct_keypair);
    }

    #[test]
    fn test_filter_current() {
        let seed = [48u8; 32];
        let mut rng = ChaChaRng::from_seed(seed);
        let keys: Vec<_> = repeat_with(Keypair::new).take(16).collect();
        let values: Vec<_> = repeat_with(|| {
            let index = rng.gen_range(0..keys.len());
            CrdsValue::new_rand(&mut rng, Some(&keys[index]))
        })
        .take(1 << 12)
        .collect();
        let mut currents = HashMap::new();
        for value in filter_current(&values) {
            // Assert that filtered values have unique labels.
            assert!(currents.insert(value.label(), value).is_none());
        }
        // Assert that currents are the most recent version of each value.
        let mut count = 0;
        for value in &values {
            let current_value = currents.get(&value.label()).unwrap();
            match value.wallclock().cmp(&current_value.wallclock()) {
                Ordering::Less => (),
                Ordering::Equal => {
                    // There is a chance that two randomly generated
                    // crds-values have the same label and wallclock.
                    if value == *current_value {
                        count += 1;
                    }
                }
                Ordering::Greater => panic!("this should not happen!"),
            }
        }
        assert_eq!(count, currents.len());
        // Currently CrdsData::new_rand is implemented for:
        //   AccountsHashes, ContactInfo, LowestSlot, LegacySnapshotHashes, Version
        //   EpochSlots x MAX_EPOCH_SLOTS
        //   Vote x MAX_VOTES
        let num_kinds = 5 + MAX_VOTES as usize + MAX_EPOCH_SLOTS as usize;
        assert!(currents.len() <= keys.len() * num_kinds);
    }

    #[test]
    fn test_node_instance_crds_lable() {
        fn make_crds_value(node: NodeInstance) -> CrdsValue {
            CrdsValue::new_unsigned(CrdsData::NodeInstance(node))
        }
        let mut rng = rand::thread_rng();
        let now = timestamp();
        let pubkey = Pubkey::new_unique();
        let node = NodeInstance::new(&mut rng, pubkey, now);
        assert_eq!(
            make_crds_value(node.clone()).label(),
            make_crds_value(node.with_wallclock(now + 8)).label()
        );
        let other = NodeInstance {
            from: Pubkey::new_unique(),
            ..node
        };
        assert_ne!(
            make_crds_value(node.clone()).label(),
            make_crds_value(other).label()
        );
        let other = NodeInstance {
            wallclock: now + 8,
            ..node
        };
        assert_eq!(
            make_crds_value(node.clone()).label(),
            make_crds_value(other).label()
        );
        let other = NodeInstance {
            timestamp: now + 8,
            ..node
        };
        assert_eq!(
            make_crds_value(node.clone()).label(),
            make_crds_value(other).label()
        );
        let other = NodeInstance {
            token: rng.gen(),
            ..node
        };
        assert_eq!(
            make_crds_value(node).label(),
            make_crds_value(other).label()
        );
    }

    #[test]
    fn test_check_duplicate_instance() {
        fn make_crds_value(node: NodeInstance) -> CrdsValue {
            CrdsValue::new_unsigned(CrdsData::NodeInstance(node))
        }
        let now = timestamp();
        let mut rng = rand::thread_rng();
        let pubkey = Pubkey::new_unique();
        let node = NodeInstance::new(&mut rng, pubkey, now);
        let node_crds = make_crds_value(node.clone());
        // Same token is not a duplicate.
        let other = NodeInstance {
            from: pubkey,
            wallclock: now + 1,
            timestamp: now + 1,
            token: node.token,
        };
        let other_crds = make_crds_value(other.clone());
        assert!(!node.check_duplicate(&other_crds));
        assert!(!other.check_duplicate(&node_crds));
        assert_eq!(node.overrides(&other_crds), None);
        assert_eq!(other.overrides(&node_crds), None);
        // Older timestamp is not a duplicate.
        let other = NodeInstance {
            from: pubkey,
            wallclock: now + 1,
            timestamp: now - 1,
            token: rng.gen(),
        };
        let other_crds = make_crds_value(other.clone());
        assert!(!node.check_duplicate(&other_crds));
        assert!(other.check_duplicate(&node_crds));
        assert_eq!(node.overrides(&other_crds), Some(true));
        assert_eq!(other.overrides(&node_crds), Some(false));
        // Updated wallclock is not a duplicate.
        let other = node.with_wallclock(now + 8);
        assert_eq!(
            other,
            NodeInstance {
                from: pubkey,
                wallclock: now + 8,
                timestamp: now,
                token: node.token,
            }
        );
        let other_crds = make_crds_value(other.clone());
        assert!(!node.check_duplicate(&other_crds));
        assert!(!other.check_duplicate(&node_crds));
        assert_eq!(node.overrides(&other_crds), None);
        assert_eq!(other.overrides(&node_crds), None);
        // Duplicate instance; tied timestamp.
        for _ in 0..10 {
            let other = NodeInstance {
                from: pubkey,
                wallclock: 0,
                timestamp: now,
                token: rng.gen(),
            };
            let other_crds = make_crds_value(other.clone());
            assert!(node.check_duplicate(&other_crds));
            assert!(other.check_duplicate(&node_crds));
            assert_eq!(node.overrides(&other_crds), Some(other.token < node.token));
            assert_eq!(other.overrides(&node_crds), Some(node.token < other.token));
        }
        // Duplicate instance; more recent timestamp.
        for _ in 0..10 {
            let other = NodeInstance {
                from: pubkey,
                wallclock: 0,
                timestamp: now + 1,
                token: rng.gen(),
            };
            let other_crds = make_crds_value(other.clone());
            assert!(node.check_duplicate(&other_crds));
            assert!(!other.check_duplicate(&node_crds));
            assert_eq!(node.overrides(&other_crds), Some(false));
            assert_eq!(other.overrides(&node_crds), Some(true));
        }
        // Different pubkey is not a duplicate.
        let other = NodeInstance {
            from: Pubkey::new_unique(),
            wallclock: now + 1,
            timestamp: now + 1,
            token: rng.gen(),
        };
        let other_crds = make_crds_value(other.clone());
        assert!(!node.check_duplicate(&other_crds));
        assert!(!other.check_duplicate(&node_crds));
        assert_eq!(node.overrides(&other_crds), None);
        assert_eq!(other.overrides(&node_crds), None);
        // Differnt crds value is not a duplicate.
        let other = LegacyContactInfo::new_rand(&mut rng, Some(pubkey));
        let other = CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(other));
        assert!(!node.check_duplicate(&other));
        assert_eq!(node.overrides(&other), None);
    }

    #[test]
    fn test_should_force_push() {
        let mut rng = rand::thread_rng();
        let pubkey = Pubkey::new_unique();
        assert!(!CrdsValue::new_unsigned(CrdsData::LegacyContactInfo(
            LegacyContactInfo::new_rand(&mut rng, Some(pubkey))
        ))
        .should_force_push(&pubkey));
        let node = CrdsValue::new_unsigned(CrdsData::NodeInstance(NodeInstance::new(
            &mut rng,
            pubkey,
            timestamp(),
        )));
        assert!(node.should_force_push(&pubkey));
        assert!(!node.should_force_push(&Pubkey::new_unique()));
    }

    fn make_rand_slots<R: Rng>(rng: &mut R) -> impl Iterator<Item = Slot> + '_ {
        repeat_with(|| rng.gen_range(1..5)).scan(0, |slot, step| {
            *slot += step;
            Some(*slot)
        })
    }

    #[test]
    fn test_restart_last_voted_fork_slots_max_bytes() {
        let keypair = Keypair::new();
        let header = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &[1, 2],
            Hash::default(),
            0,
        )
        .unwrap();
        // If the following assert fails, please update RestartLastVotedForkSlots::MAX_BYTES
        assert_eq!(
            RestartLastVotedForkSlots::MAX_BYTES,
            MAX_CRDS_OBJECT_SIZE - serialized_size(&header).unwrap() as usize
        );

        // Create large enough slots to make sure we are discarding some to make slots fit.
        let mut rng = rand::thread_rng();
        let large_length = 8000;
        let range: Vec<Slot> = make_rand_slots(&mut rng).take(large_length).collect();
        let large_slots = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &range,
            Hash::default(),
            0,
        )
        .unwrap();
        assert!(serialized_size(&large_slots).unwrap() <= MAX_CRDS_OBJECT_SIZE as u64);
        let retrieved_slots = large_slots.to_slots(0);
        assert!(retrieved_slots.len() <= range.len());
        assert!(retrieved_slots.last().unwrap() - retrieved_slots.first().unwrap() > 5000);
    }

    #[test]
    fn test_restart_last_voted_fork_slots() {
        let keypair = Keypair::new();
        let slot = 53;
        let slot_parent = slot - 5;
        let shred_version = 21;
        let original_slots_vec = [slot_parent, slot];
        let slots = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &original_slots_vec,
            Hash::default(),
            shred_version,
        )
        .unwrap();
        let value =
            CrdsValue::new_signed(CrdsData::RestartLastVotedForkSlots(slots.clone()), &keypair);
        assert_eq!(value.sanitize(), Ok(()));
        let label = value.label();
        assert_eq!(
            label,
            CrdsValueLabel::RestartLastVotedForkSlots(keypair.pubkey())
        );
        assert_eq!(label.pubkey(), keypair.pubkey());
        assert_eq!(value.wallclock(), slots.wallclock);
        let retrieved_slots = slots.to_slots(0);
        assert_eq!(retrieved_slots.len(), 2);
        assert_eq!(retrieved_slots[0], slot_parent);
        assert_eq!(retrieved_slots[1], slot);

        let bad_value = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &[],
            Hash::default(),
            shred_version,
        );
        assert!(bad_value.is_err());

        let last_slot: Slot = 8000;
        let large_slots_vec: Vec<Slot> = (0..last_slot + 1).collect();
        let large_slots = RestartLastVotedForkSlots::new(
            keypair.pubkey(),
            timestamp(),
            &large_slots_vec,
            Hash::default(),
            shred_version,
        )
        .unwrap();
        assert!(serialized_size(&large_slots).unwrap() < MAX_CRDS_OBJECT_SIZE as u64);
        let retrieved_slots = large_slots.to_slots(0);
        assert_eq!(retrieved_slots, large_slots_vec);
    }

    fn check_run_length_encoding(slots: Vec<Slot>) {
        let last_voted_slot = slots[slots.len() - 1];
        let mut bitvec = BitVec::new_fill(false, last_voted_slot - slots[0] + 1);
        for slot in &slots {
            bitvec.set(last_voted_slot - slot, true);
        }
        let rle = RunLengthEncoding::new(&bitvec);
        let retrieved_slots = rle.to_slots(last_voted_slot, 0);
        assert_eq!(retrieved_slots, slots);
    }

    #[test]
    fn test_run_length_encoding() {
        check_run_length_encoding((1000..16384 + 1000).map(|x| x as Slot).collect_vec());
        check_run_length_encoding([1000 as Slot].into());
        check_run_length_encoding(
            [
                1000 as Slot,
                RestartLastVotedForkSlots::MAX_SLOTS as Slot + 999,
            ]
            .into(),
        );
        check_run_length_encoding((1000..1800).step_by(2).map(|x| x as Slot).collect_vec());

        let mut rng = rand::thread_rng();
        let large_length = 500;
        let range: Vec<Slot> = make_rand_slots(&mut rng).take(large_length).collect();
        check_run_length_encoding(range);
    }
}
