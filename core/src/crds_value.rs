use crate::contact_info::ContactInfo;
use crate::deprecated;
use crate::epoch_slots::EpochSlots;
use bincode::{serialize, serialized_size};
use solana_sdk::sanitize::{Sanitize, SanitizeError};
use solana_sdk::timing::timestamp;
use solana_sdk::{
    clock::Slot,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signable, Signature},
    transaction::Transaction,
};
use std::{
    borrow::{Borrow, Cow},
    collections::{BTreeSet, HashSet},
    fmt,
};

pub const MAX_WALLCLOCK: u64 = 1_000_000_000_000_000;
pub const MAX_SLOT: u64 = 1_000_000_000_000_000;

pub type VoteIndex = u8;
pub const MAX_VOTES: VoteIndex = 32;

pub type EpochSlotsIndex = u8;
pub const MAX_EPOCH_SLOTS: EpochSlotsIndex = 255;

/// CrdsValue that is replicated across the cluster
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
            .verify(&self.pubkey().as_ref(), self.signable_data().borrow())
    }
}

/// CrdsData that defines the different types of items CrdsValues can hold
/// * Merge Strategy - Latest wallclock is picked
/// * LowestSlot index is deprecated
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CrdsData {
    ContactInfo(ContactInfo),
    Vote(VoteIndex, Vote),
    LowestSlot(u8, LowestSlot),
    SnapshotHashes(SnapshotHash),
    AccountsHashes(SnapshotHash),
    EpochSlots(EpochSlotsIndex, EpochSlots),
    Version(Version),
}

impl Sanitize for CrdsData {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        match self {
            CrdsData::ContactInfo(val) => val.sanitize(),
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
            CrdsData::SnapshotHashes(val) => val.sanitize(),
            CrdsData::AccountsHashes(val) => val.sanitize(),
            CrdsData::EpochSlots(ix, val) => {
                if *ix as usize >= MAX_EPOCH_SLOTS as usize {
                    return Err(SanitizeError::ValueOutOfBounds);
                }
                val.sanitize()
            }
            CrdsData::Version(version) => version.sanitize(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SnapshotHash {
    pub from: Pubkey,
    pub hashes: Vec<(Slot, Hash)>,
    pub wallclock: u64,
}

impl Sanitize for SnapshotHash {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        for (slot, _) in &self.hashes {
            if *slot >= MAX_SLOT {
                return Err(SanitizeError::ValueOutOfBounds);
            }
        }
        self.from.sanitize()
    }
}

impl SnapshotHash {
    pub fn new(from: Pubkey, hashes: Vec<(Slot, Hash)>) -> Self {
        Self {
            from,
            hashes,
            wallclock: timestamp(),
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
}

impl Sanitize for LowestSlot {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vote {
    pub from: Pubkey,
    pub transaction: Transaction,
    pub wallclock: u64,
}

impl Sanitize for Vote {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        self.from.sanitize()?;
        self.transaction.sanitize()
    }
}

impl Vote {
    pub fn new(from: &Pubkey, transaction: Transaction, wallclock: u64) -> Self {
        Self {
            from: *from,
            transaction,
            wallclock,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Version {
    pub from: Pubkey,
    pub wallclock: u64,
    pub version: solana_version::Version,
}

impl Sanitize for Version {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        self.from.sanitize()?;
        self.version.sanitize()
    }
}

impl Version {
    pub fn new(from: Pubkey) -> Self {
        Self {
            from,
            wallclock: timestamp(),
            version: solana_version::Version::default(),
        }
    }
}

/// Type of the replicated value
/// These are labels for values in a record that is associated with `Pubkey`
#[derive(PartialEq, Hash, Eq, Clone, Debug)]
pub enum CrdsValueLabel {
    ContactInfo(Pubkey),
    Vote(VoteIndex, Pubkey),
    LowestSlot(Pubkey),
    SnapshotHashes(Pubkey),
    EpochSlots(EpochSlotsIndex, Pubkey),
    AccountsHashes(Pubkey),
    Version(Pubkey),
}

impl fmt::Display for CrdsValueLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrdsValueLabel::ContactInfo(_) => write!(f, "ContactInfo({})", self.pubkey()),
            CrdsValueLabel::Vote(ix, _) => write!(f, "Vote({}, {})", ix, self.pubkey()),
            CrdsValueLabel::LowestSlot(_) => write!(f, "LowestSlot({})", self.pubkey()),
            CrdsValueLabel::SnapshotHashes(_) => write!(f, "SnapshotHash({})", self.pubkey()),
            CrdsValueLabel::EpochSlots(ix, _) => write!(f, "EpochSlots({}, {})", ix, self.pubkey()),
            CrdsValueLabel::AccountsHashes(_) => write!(f, "AccountsHashes({})", self.pubkey()),
            CrdsValueLabel::Version(_) => write!(f, "Version({})", self.pubkey()),
        }
    }
}

impl CrdsValueLabel {
    pub fn pubkey(&self) -> Pubkey {
        match self {
            CrdsValueLabel::ContactInfo(p) => *p,
            CrdsValueLabel::Vote(_, p) => *p,
            CrdsValueLabel::LowestSlot(p) => *p,
            CrdsValueLabel::SnapshotHashes(p) => *p,
            CrdsValueLabel::EpochSlots(_, p) => *p,
            CrdsValueLabel::AccountsHashes(p) => *p,
            CrdsValueLabel::Version(p) => *p,
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
    /// Totally unsecure unverifiable wallclock of the node that generated this message
    /// Latest wallclock is always picked.
    /// This is used to time out push messages.
    pub fn wallclock(&self) -> u64 {
        match &self.data {
            CrdsData::ContactInfo(contact_info) => contact_info.wallclock,
            CrdsData::Vote(_, vote) => vote.wallclock,
            CrdsData::LowestSlot(_, obj) => obj.wallclock,
            CrdsData::SnapshotHashes(hash) => hash.wallclock,
            CrdsData::AccountsHashes(hash) => hash.wallclock,
            CrdsData::EpochSlots(_, p) => p.wallclock,
            CrdsData::Version(version) => version.wallclock,
        }
    }
    pub fn pubkey(&self) -> Pubkey {
        match &self.data {
            CrdsData::ContactInfo(contact_info) => contact_info.id,
            CrdsData::Vote(_, vote) => vote.from,
            CrdsData::LowestSlot(_, slots) => slots.from,
            CrdsData::SnapshotHashes(hash) => hash.from,
            CrdsData::AccountsHashes(hash) => hash.from,
            CrdsData::EpochSlots(_, p) => p.from,
            CrdsData::Version(version) => version.from,
        }
    }
    pub fn label(&self) -> CrdsValueLabel {
        match &self.data {
            CrdsData::ContactInfo(_) => CrdsValueLabel::ContactInfo(self.pubkey()),
            CrdsData::Vote(ix, _) => CrdsValueLabel::Vote(*ix, self.pubkey()),
            CrdsData::LowestSlot(_, _) => CrdsValueLabel::LowestSlot(self.pubkey()),
            CrdsData::SnapshotHashes(_) => CrdsValueLabel::SnapshotHashes(self.pubkey()),
            CrdsData::AccountsHashes(_) => CrdsValueLabel::AccountsHashes(self.pubkey()),
            CrdsData::EpochSlots(ix, _) => CrdsValueLabel::EpochSlots(*ix, self.pubkey()),
            CrdsData::Version(_) => CrdsValueLabel::Version(self.pubkey()),
        }
    }
    pub fn contact_info(&self) -> Option<&ContactInfo> {
        match &self.data {
            CrdsData::ContactInfo(contact_info) => Some(contact_info),
            _ => None,
        }
    }
    pub fn vote(&self) -> Option<&Vote> {
        match &self.data {
            CrdsData::Vote(_, vote) => Some(vote),
            _ => None,
        }
    }

    pub fn vote_index(&self) -> Option<VoteIndex> {
        match &self.data {
            CrdsData::Vote(ix, _) => Some(*ix),
            _ => None,
        }
    }

    pub fn lowest_slot(&self) -> Option<&LowestSlot> {
        match &self.data {
            CrdsData::LowestSlot(_, slots) => Some(slots),
            _ => None,
        }
    }

    pub fn snapshot_hash(&self) -> Option<&SnapshotHash> {
        match &self.data {
            CrdsData::SnapshotHashes(slots) => Some(slots),
            _ => None,
        }
    }

    pub fn accounts_hash(&self) -> Option<&SnapshotHash> {
        match &self.data {
            CrdsData::AccountsHashes(slots) => Some(slots),
            _ => None,
        }
    }

    pub fn epoch_slots(&self) -> Option<&EpochSlots> {
        match &self.data {
            CrdsData::EpochSlots(_, slots) => Some(slots),
            _ => None,
        }
    }

    pub fn version(&self) -> Option<&Version> {
        match &self.data {
            CrdsData::Version(version) => Some(version),
            _ => None,
        }
    }

    /// Return all the possible labels for a record identified by Pubkey.
    pub fn record_labels(key: &Pubkey) -> Vec<CrdsValueLabel> {
        let mut labels = vec![
            CrdsValueLabel::ContactInfo(*key),
            CrdsValueLabel::LowestSlot(*key),
            CrdsValueLabel::SnapshotHashes(*key),
            CrdsValueLabel::AccountsHashes(*key),
            CrdsValueLabel::Version(*key),
        ];
        labels.extend((0..MAX_VOTES).map(|ix| CrdsValueLabel::Vote(ix, *key)));
        labels.extend((0..MAX_EPOCH_SLOTS).map(|ix| CrdsValueLabel::EpochSlots(ix, *key)));
        labels
    }

    /// Returns the size (in bytes) of a CrdsValue
    pub fn size(&self) -> u64 {
        serialized_size(&self).expect("unable to serialize contact info")
    }

    pub fn compute_vote_index(tower_index: usize, mut votes: Vec<&CrdsValue>) -> VoteIndex {
        let mut available: HashSet<VoteIndex> = (0..MAX_VOTES).collect();
        votes.iter().filter_map(|v| v.vote_index()).for_each(|ix| {
            available.remove(&ix);
        });

        // free index
        if !available.is_empty() {
            return *available.iter().next().unwrap();
        }

        assert!(votes.len() == MAX_VOTES as usize);
        votes.sort_by_key(|v| v.vote().expect("all values must be votes").wallclock);

        // If Tower is full, oldest removed first
        if tower_index + 1 == MAX_VOTES as usize {
            return votes[0].vote_index().expect("all values must be votes");
        }

        // If Tower is not full, the early votes have expired
        assert!(tower_index < MAX_VOTES as usize);

        votes[tower_index]
            .vote_index()
            .expect("all values must be votes")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::contact_info::ContactInfo;
    use bincode::deserialize;
    use solana_perf::test_tx::test_tx;
    use solana_sdk::signature::{Keypair, Signer};
    use solana_sdk::timing::timestamp;

    #[test]
    fn test_labels() {
        let mut hits = [false; 5 + MAX_VOTES as usize + MAX_EPOCH_SLOTS as usize];
        // this method should cover all the possible labels
        for v in &CrdsValue::record_labels(&Pubkey::default()) {
            match v {
                CrdsValueLabel::ContactInfo(_) => hits[0] = true,
                CrdsValueLabel::LowestSlot(_) => hits[1] = true,
                CrdsValueLabel::SnapshotHashes(_) => hits[2] = true,
                CrdsValueLabel::AccountsHashes(_) => hits[3] = true,
                CrdsValueLabel::Version(_) => hits[4] = true,
                CrdsValueLabel::Vote(ix, _) => hits[*ix as usize + 5] = true,
                CrdsValueLabel::EpochSlots(ix, _) => {
                    hits[*ix as usize + MAX_VOTES as usize + 5] = true
                }
            }
        }
        assert!(hits.iter().all(|x| *x));
    }
    #[test]
    fn test_keys_and_values() {
        let v = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(v.wallclock(), 0);
        let key = v.contact_info().unwrap().id;
        assert_eq!(v.label(), CrdsValueLabel::ContactInfo(key));

        let v = CrdsValue::new_unsigned(CrdsData::Vote(
            0,
            Vote::new(&Pubkey::default(), test_tx(), 0),
        ));
        assert_eq!(v.wallclock(), 0);
        let key = v.vote().unwrap().from;
        assert_eq!(v.label(), CrdsValueLabel::Vote(0, key));

        let v = CrdsValue::new_unsigned(CrdsData::LowestSlot(
            0,
            LowestSlot::new(Pubkey::default(), 0, 0),
        ));
        assert_eq!(v.wallclock(), 0);
        let key = v.lowest_slot().unwrap().from;
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
        let keypair = Keypair::new();
        let wrong_keypair = Keypair::new();
        let mut v = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &keypair.pubkey(),
            timestamp(),
        )));
        verify_signatures(&mut v, &keypair, &wrong_keypair);
        v = CrdsValue::new_unsigned(CrdsData::Vote(
            0,
            Vote::new(&keypair.pubkey(), test_tx(), timestamp()),
        ));
        verify_signatures(&mut v, &keypair, &wrong_keypair);
        v = CrdsValue::new_unsigned(CrdsData::LowestSlot(
            0,
            LowestSlot::new(keypair.pubkey(), 0, timestamp()),
        ));
        verify_signatures(&mut v, &keypair, &wrong_keypair);
    }

    #[test]
    fn test_max_vote_index() {
        let keypair = Keypair::new();
        let vote = CrdsValue::new_signed(
            CrdsData::Vote(
                MAX_VOTES,
                Vote::new(&keypair.pubkey(), test_tx(), timestamp()),
            ),
            &keypair,
        );
        assert!(vote.sanitize().is_err());
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
    fn test_compute_vote_index_empty() {
        for i in 0..MAX_VOTES {
            let votes = vec![];
            assert!(CrdsValue::compute_vote_index(i as usize, votes) < MAX_VOTES);
        }
    }

    #[test]
    fn test_compute_vote_index_one() {
        let keypair = Keypair::new();
        let vote = CrdsValue::new_unsigned(CrdsData::Vote(
            0,
            Vote::new(&keypair.pubkey(), test_tx(), 0),
        ));
        for i in 0..MAX_VOTES {
            let votes = vec![&vote];
            assert!(CrdsValue::compute_vote_index(i as usize, votes) > 0);
            let votes = vec![&vote];
            assert!(CrdsValue::compute_vote_index(i as usize, votes) < MAX_VOTES);
        }
    }

    #[test]
    fn test_compute_vote_index_full() {
        let keypair = Keypair::new();
        let votes: Vec<_> = (0..MAX_VOTES)
            .map(|x| {
                CrdsValue::new_unsigned(CrdsData::Vote(
                    x,
                    Vote::new(&keypair.pubkey(), test_tx(), x as u64),
                ))
            })
            .collect();
        let vote_refs = votes.iter().collect();
        //pick the oldest vote when full
        assert_eq!(CrdsValue::compute_vote_index(31, vote_refs), 0);
        //pick the index
        let vote_refs = votes.iter().collect();
        assert_eq!(CrdsValue::compute_vote_index(0, vote_refs), 0);
        let vote_refs = votes.iter().collect();
        assert_eq!(CrdsValue::compute_vote_index(30, vote_refs), 30);
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
        value.sign(&correct_keypair);
        assert!(value.verify());
        value.sign(&wrong_keypair);
        assert!(!value.verify());
        serialize_deserialize_value(value, correct_keypair);
    }
}
