use {
    crate::{
        cluster_info::MAX_ACCOUNTS_HASHES,
        contact_info::ContactInfo,
        deprecated,
        duplicate_shred::{DuplicateShred, DuplicateShredIndex, MAX_DUPLICATE_SHREDS},
        epoch_slots::EpochSlots,
        legacy_contact_info::LegacyContactInfo,
        restart_crds_values::{RestartHeaviestFork, RestartLastVotedForkSlots},
    },
    rand::{CryptoRng, Rng},
    serde::de::{Deserialize, Deserializer},
    solana_sanitize::{Sanitize, SanitizeError},
    solana_sdk::{
        clock::Slot,
        hash::Hash,
        pubkey::{self, Pubkey},
        timing::timestamp,
        transaction::Transaction,
    },
    solana_vote::vote_parser,
    std::{cmp::Ordering, collections::BTreeSet},
};

pub(crate) const MAX_WALLCLOCK: u64 = 1_000_000_000_000_000;
pub(crate) const MAX_SLOT: u64 = 1_000_000_000_000_000;

pub(crate) type VoteIndex = u8;
// TODO: Remove this in favor of vote_state::MAX_LOCKOUT_HISTORY once
// the fleet is updated to the new ClusterInfo::push_vote code.
const MAX_VOTES: VoteIndex = 32;

pub(crate) type EpochSlotsIndex = u8;
pub(crate) const MAX_EPOCH_SLOTS: EpochSlotsIndex = 255;

/// CrdsData that defines the different types of items CrdsValues can hold
/// * Merge Strategy - Latest wallclock is picked
/// * LowestSlot index is deprecated
#[allow(clippy::large_enum_variant)]
#[cfg_attr(feature = "frozen-abi", derive(AbiExample, AbiEnumVisitor))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum CrdsData {
    #[allow(private_interfaces)]
    LegacyContactInfo(LegacyContactInfo),
    Vote(VoteIndex, Vote),
    LowestSlot(/*DEPRECATED:*/ u8, LowestSlot),
    #[allow(private_interfaces)]
    LegacySnapshotHashes(LegacySnapshotHashes), // Deprecated
    #[allow(private_interfaces)]
    AccountsHashes(AccountsHashes), // Deprecated
    EpochSlots(EpochSlotsIndex, EpochSlots),
    #[allow(private_interfaces)]
    LegacyVersion(LegacyVersion),
    #[allow(private_interfaces)]
    Version(Version),
    #[allow(private_interfaces)]
    NodeInstance(NodeInstance),
    DuplicateShred(DuplicateShredIndex, DuplicateShred),
    SnapshotHashes(SnapshotHashes),
    ContactInfo(ContactInfo),
    RestartLastVotedForkSlots(RestartLastVotedForkSlots),
    RestartHeaviestFork(RestartHeaviestFork),
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
            CrdsData::RestartHeaviestFork(fork) => fork.sanitize(),
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
    pub(crate) fn new_rand<R: Rng>(rng: &mut R, pubkey: Option<Pubkey>) -> CrdsData {
        let kind = rng.gen_range(0..9);
        // TODO: Implement other kinds of CrdsData here.
        // TODO: Assign ranges to each arm proportional to their frequency in
        // the mainnet crds table.
        match kind {
            0 => CrdsData::ContactInfo(ContactInfo::new_rand(rng, pubkey)),
            // Index for LowestSlot is deprecated and should be zero.
            1 => CrdsData::LowestSlot(0, LowestSlot::new_rand(rng, pubkey)),
            2 => CrdsData::LegacySnapshotHashes(LegacySnapshotHashes::new_rand(rng, pubkey)),
            3 => CrdsData::AccountsHashes(AccountsHashes::new_rand(rng, pubkey)),
            4 => CrdsData::Version(Version::new_rand(rng, pubkey)),
            5 => CrdsData::Vote(rng.gen_range(0..MAX_VOTES), Vote::new_rand(rng, pubkey)),
            6 => CrdsData::RestartLastVotedForkSlots(RestartLastVotedForkSlots::new_rand(
                rng, pubkey,
            )),
            7 => CrdsData::RestartHeaviestFork(RestartHeaviestFork::new_rand(rng, pubkey)),
            _ => CrdsData::EpochSlots(
                rng.gen_range(0..MAX_EPOCH_SLOTS),
                EpochSlots::new_rand(rng, pubkey),
            ),
        }
    }

    pub(crate) fn wallclock(&self) -> u64 {
        match self {
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
            CrdsData::RestartHeaviestFork(fork) => fork.wallclock,
        }
    }

    pub(crate) fn pubkey(&self) -> Pubkey {
        match &self {
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
            CrdsData::RestartHeaviestFork(fork) => fork.from,
        }
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccountsHashes {
    pub(crate) from: Pubkey,
    pub(crate) hashes: Vec<(Slot, Hash)>,
    pub(crate) wallclock: u64,
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

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LowestSlot {
    pub(crate) from: Pubkey,
    root: Slot, //deprecated
    pub lowest: Slot,
    slots: BTreeSet<Slot>,                        //deprecated
    stash: Vec<deprecated::EpochIncompleteSlots>, //deprecated
    wallclock: u64,
}

impl LowestSlot {
    pub(crate) fn new(from: Pubkey, lowest: Slot, wallclock: u64) -> Self {
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

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyVersion {
    from: Pubkey,
    wallclock: u64,
    pub(crate) version: solana_version::LegacyVersion1,
}

impl Sanitize for LegacyVersion {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.from.sanitize()?;
        self.version.sanitize()
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Version {
    from: Pubkey,
    wallclock: u64,
    pub(crate) version: solana_version::LegacyVersion2,
}

impl Sanitize for Version {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        self.from.sanitize()?;
        self.version.sanitize()
    }
}

impl Version {
    pub(crate) fn new(from: Pubkey) -> Self {
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

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct NodeInstance {
    from: Pubkey,
    wallclock: u64,
    timestamp: u64, // Timestamp when the instance was created.
    token: u64,     // Randomly generated value at node instantiation.
}

impl NodeInstance {
    pub(crate) fn new<R>(rng: &mut R, from: Pubkey, now: u64) -> Self
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
    pub(crate) fn check_duplicate(&self, other: &NodeInstance) -> bool {
        self.token != other.token && self.timestamp <= other.timestamp && self.from == other.from
    }

    // Returns None if tokens are the same or other is not a node-instance from
    // the same owner. Otherwise returns true if self has more recent timestamp
    // than other, and so overrides it.
    pub(crate) fn overrides(&self, other: &NodeInstance) -> Option<bool> {
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
        crate::crds_value::CrdsValue,
        bincode::Options,
        solana_perf::test_tx::new_test_vote_tx,
        solana_sdk::{
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_vote_program::{vote_instruction, vote_state},
    };

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

    #[test]
    fn test_node_instance_crds_label() {
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
        let now = timestamp();
        let mut rng = rand::thread_rng();
        let pubkey = Pubkey::new_unique();
        let node = NodeInstance::new(&mut rng, pubkey, now);
        // Same token is not a duplicate.
        let other = NodeInstance {
            from: pubkey,
            wallclock: now + 1,
            timestamp: now + 1,
            token: node.token,
        };
        assert!(!node.check_duplicate(&other));
        assert!(!other.check_duplicate(&node));
        assert_eq!(node.overrides(&other), None);
        assert_eq!(other.overrides(&node), None);
        // Older timestamp is not a duplicate.
        let other = NodeInstance {
            from: pubkey,
            wallclock: now + 1,
            timestamp: now - 1,
            token: rng.gen(),
        };
        assert!(!node.check_duplicate(&other));
        assert!(other.check_duplicate(&node));
        assert_eq!(node.overrides(&other), Some(true));
        assert_eq!(other.overrides(&node), Some(false));
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
        assert!(!node.check_duplicate(&other));
        assert!(!other.check_duplicate(&node));
        assert_eq!(node.overrides(&other), None);
        assert_eq!(other.overrides(&node), None);
        // Duplicate instance; tied timestamp.
        for _ in 0..10 {
            let other = NodeInstance {
                from: pubkey,
                wallclock: 0,
                timestamp: now,
                token: rng.gen(),
            };
            assert!(node.check_duplicate(&other));
            assert!(other.check_duplicate(&node));
            assert_eq!(node.overrides(&other), Some(other.token < node.token));
            assert_eq!(other.overrides(&node), Some(node.token < other.token));
        }
        // Duplicate instance; more recent timestamp.
        for _ in 0..10 {
            let other = NodeInstance {
                from: pubkey,
                wallclock: 0,
                timestamp: now + 1,
                token: rng.gen(),
            };
            assert!(node.check_duplicate(&other));
            assert!(!other.check_duplicate(&node));
            assert_eq!(node.overrides(&other), Some(false));
            assert_eq!(other.overrides(&node), Some(true));
        }
        // Different pubkey is not a duplicate.
        let other = NodeInstance {
            from: Pubkey::new_unique(),
            wallclock: now + 1,
            timestamp: now + 1,
            token: rng.gen(),
        };
        assert!(!node.check_duplicate(&other));
        assert!(!other.check_duplicate(&node));
        assert_eq!(node.overrides(&other), None);
        assert_eq!(other.overrides(&node), None);
    }
}
