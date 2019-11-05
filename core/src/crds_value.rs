use crate::contact_info::ContactInfo;
use bincode::{serialize, serialized_size};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signable, Signature};
use solana_sdk::transaction::Transaction;
use std::borrow::Borrow;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::fmt;

pub type VoteIndex = u8;
pub const MAX_VOTES: VoteIndex = 32;

/// CrdsValue that is replicated across the cluster
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CrdsValue {
    pub signature: Signature,
    pub data: CrdsData,
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
        let sig_check = self
            .get_signature()
            .verify(&self.pubkey().as_ref(), self.signable_data().borrow());
        let data_check = match &self.data {
            CrdsData::Vote(ix, _) => *ix < MAX_VOTES,
            _ => true,
        };
        sig_check && data_check
    }
}

/// CrdsData that defines the different types of items CrdsValues can hold
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CrdsData {
    /// * Merge Strategy - Latest wallclock is picked
    ContactInfo(ContactInfo),
    /// * Merge Strategy - Latest wallclock is picked
    Vote(VoteIndex, Vote),
    /// * Merge Strategy - Latest wallclock is picked
    EpochSlots(EpochSlots),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EpochSlots {
    pub from: Pubkey,
    pub root: u64,
    pub slots: BTreeSet<u64>,
    pub wallclock: u64,
}

impl EpochSlots {
    pub fn new(from: Pubkey, root: u64, slots: BTreeSet<u64>, wallclock: u64) -> Self {
        Self {
            from,
            root,
            slots,
            wallclock,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vote {
    pub from: Pubkey,
    pub transaction: Transaction,
    pub wallclock: u64,
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

/// Type of the replicated value
/// These are labels for values in a record that is associated with `Pubkey`
#[derive(PartialEq, Hash, Eq, Clone, Debug)]
pub enum CrdsValueLabel {
    ContactInfo(Pubkey),
    Vote(VoteIndex, Pubkey),
    EpochSlots(Pubkey),
}

impl fmt::Display for CrdsValueLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrdsValueLabel::ContactInfo(_) => write!(f, "ContactInfo({})", self.pubkey()),
            CrdsValueLabel::Vote(ix, _) => write!(f, "Vote({}, {})", ix, self.pubkey()),
            CrdsValueLabel::EpochSlots(_) => write!(f, "EpochSlots({})", self.pubkey()),
        }
    }
}

impl CrdsValueLabel {
    pub fn pubkey(&self) -> Pubkey {
        match self {
            CrdsValueLabel::ContactInfo(p) => *p,
            CrdsValueLabel::Vote(_, p) => *p,
            CrdsValueLabel::EpochSlots(p) => *p,
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
    /// Totally unsecure unverfiable wallclock of the node that generated this message
    /// Latest wallclock is always picked.
    /// This is used to time out push messages.
    pub fn wallclock(&self) -> u64 {
        match &self.data {
            CrdsData::ContactInfo(contact_info) => contact_info.wallclock,
            CrdsData::Vote(_, vote) => vote.wallclock,
            CrdsData::EpochSlots(vote) => vote.wallclock,
        }
    }
    pub fn pubkey(&self) -> Pubkey {
        match &self.data {
            CrdsData::ContactInfo(contact_info) => contact_info.id,
            CrdsData::Vote(_, vote) => vote.from,
            CrdsData::EpochSlots(slots) => slots.from,
        }
    }
    pub fn label(&self) -> CrdsValueLabel {
        match &self.data {
            CrdsData::ContactInfo(_) => CrdsValueLabel::ContactInfo(self.pubkey()),
            CrdsData::Vote(ix, _) => CrdsValueLabel::Vote(*ix, self.pubkey()),
            CrdsData::EpochSlots(_) => CrdsValueLabel::EpochSlots(self.pubkey()),
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

    pub fn epoch_slots(&self) -> Option<&EpochSlots> {
        match &self.data {
            CrdsData::EpochSlots(slots) => Some(slots),
            _ => None,
        }
    }
    /// Return all the possible labels for a record identified by Pubkey.
    pub fn record_labels(key: &Pubkey) -> Vec<CrdsValueLabel> {
        let mut labels = vec![
            CrdsValueLabel::ContactInfo(*key),
            CrdsValueLabel::EpochSlots(*key),
        ];
        labels.extend((0..MAX_VOTES).map(|ix| CrdsValueLabel::Vote(ix, *key)));
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
    use crate::test_tx::test_tx;
    use bincode::deserialize;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::timing::timestamp;

    #[test]
    fn test_labels() {
        let mut hits = [false; 2 + MAX_VOTES as usize];
        // this method should cover all the possible labels
        for v in &CrdsValue::record_labels(&Pubkey::default()) {
            match v {
                CrdsValueLabel::ContactInfo(_) => hits[0] = true,
                CrdsValueLabel::EpochSlots(_) => hits[1] = true,
                CrdsValueLabel::Vote(ix, _) => hits[*ix as usize + 2] = true,
            }
        }
        assert!(hits.iter().all(|x| *x));
    }
    #[test]
    fn test_keys_and_values() {
        let v = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(v.wallclock(), 0);
        let key = v.clone().contact_info().unwrap().id;
        assert_eq!(v.label(), CrdsValueLabel::ContactInfo(key));

        let v = CrdsValue::new_unsigned(CrdsData::Vote(
            0,
            Vote::new(&Pubkey::default(), test_tx(), 0),
        ));
        assert_eq!(v.wallclock(), 0);
        let key = v.clone().vote().unwrap().from;
        assert_eq!(v.label(), CrdsValueLabel::Vote(0, key));

        let v = CrdsValue::new_unsigned(CrdsData::EpochSlots(EpochSlots::new(
            Pubkey::default(),
            0,
            BTreeSet::new(),
            0,
        )));
        assert_eq!(v.wallclock(), 0);
        let key = v.clone().epoch_slots().unwrap().from;
        assert_eq!(v.label(), CrdsValueLabel::EpochSlots(key));
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
        let btreeset: BTreeSet<u64> = vec![1, 2, 3, 6, 8].into_iter().collect();
        v = CrdsValue::new_unsigned(CrdsData::EpochSlots(EpochSlots::new(
            keypair.pubkey(),
            0,
            btreeset,
            timestamp(),
        )));
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
        assert!(!vote.verify());
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
