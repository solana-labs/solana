use bincode::serialize;
use contact_info::ContactInfo;
use signature::{Keypair, Signature};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::fmt;

/// CrdsValue that is replicated across the cluster
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CrdsValue {
    /// * Merge Strategy - Latest wallclock is picked
    ContactInfo(ContactInfo),
    /// TODO, Votes need a height potentially in the userdata
    /// * Merge Strategy - Latest height is picked
    Vote(Vote),
    /// * Merge Strategy - Latest wallclock is picked
    LeaderId(LeaderId),
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct LeaderId {
    pub id: Pubkey,
    pub signature: Signature,
    pub leader_id: Pubkey,
    pub wallclock: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vote {
    pub transaction: Transaction,
    pub signature: Signature,
    pub height: u64,
    pub wallclock: u64,
}

/// Type of the replicated value
/// These are labels for values in a record that is associated with `Pubkey`
#[derive(PartialEq, Hash, Eq, Clone, Debug)]
pub enum CrdsValueLabel {
    ContactInfo(Pubkey),
    Vote(Pubkey),
    LeaderId(Pubkey),
}

impl fmt::Display for CrdsValueLabel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CrdsValueLabel::ContactInfo(_) => write!(f, "ContactInfo({})", self.pubkey()),
            CrdsValueLabel::Vote(_) => write!(f, "Vote({})", self.pubkey()),
            CrdsValueLabel::LeaderId(_) => write!(f, "LeaderId({})", self.pubkey()),
        }
    }
}

impl CrdsValueLabel {
    pub fn pubkey(&self) -> Pubkey {
        match self {
            CrdsValueLabel::ContactInfo(p) => *p,
            CrdsValueLabel::Vote(p) => *p,
            CrdsValueLabel::LeaderId(p) => *p,
        }
    }
}

impl LeaderId {
    pub fn new(id: Pubkey, leader_id: Pubkey, wallclock: u64) -> Self {
        LeaderId {
            id,
            signature: Signature::default(),
            leader_id,
            wallclock,
        }
    }
}

impl Vote {
    pub fn new(transaction: Transaction, height: u64, wallclock: u64) -> Self {
        Vote {
            transaction,
            signature: Signature::default(),
            height,
            wallclock,
        }
    }
}

impl CrdsValue {
    /// Totally unsecure unverfiable wallclock of the node that generated this message
    /// Latest wallclock is always picked.
    /// This is used to time out push messages.
    pub fn wallclock(&self) -> u64 {
        match self {
            CrdsValue::ContactInfo(contact_info) => contact_info.wallclock,
            CrdsValue::Vote(vote) => vote.wallclock,
            CrdsValue::LeaderId(leader_id) => leader_id.wallclock,
        }
    }
    pub fn label(&self) -> CrdsValueLabel {
        match self {
            CrdsValue::ContactInfo(contact_info) => CrdsValueLabel::ContactInfo(contact_info.id),
            CrdsValue::Vote(vote) => CrdsValueLabel::Vote(vote.transaction.account_keys[0]),
            CrdsValue::LeaderId(leader_id) => CrdsValueLabel::LeaderId(leader_id.id),
        }
    }
    pub fn contact_info(&self) -> Option<&ContactInfo> {
        match self {
            CrdsValue::ContactInfo(contact_info) => Some(contact_info),
            _ => None,
        }
    }
    pub fn leader_id(&self) -> Option<&LeaderId> {
        match self {
            CrdsValue::LeaderId(leader_id) => Some(leader_id),
            _ => None,
        }
    }
    pub fn vote(&self) -> Option<&Vote> {
        match self {
            CrdsValue::Vote(vote) => Some(vote),
            _ => None,
        }
    }
    /// Return all the possible labels for a record identified by Pubkey.
    pub fn record_labels(key: Pubkey) -> [CrdsValueLabel; 3] {
        [
            CrdsValueLabel::ContactInfo(key),
            CrdsValueLabel::Vote(key),
            CrdsValueLabel::LeaderId(key),
        ]
    }

    pub fn sign(&mut self, keypair: &Keypair) {
        let sign_data = self.get_sign_data();
        let signature = Signature::new(&keypair.sign(&sign_data).as_ref());
        match self {
            CrdsValue::ContactInfo(contact_info) => contact_info.signature = signature,
            CrdsValue::Vote(vote) => vote.signature = signature,
            CrdsValue::LeaderId(leader_id) => leader_id.signature = signature,
        }
    }

    pub fn verify_signature(&self) -> bool {
        let sig = match self {
            CrdsValue::ContactInfo(contact_info) => contact_info.signature,
            CrdsValue::Vote(vote) => vote.signature,
            CrdsValue::LeaderId(leader_id) => leader_id.signature,
        };
        sig.verify(&self.label().pubkey().as_ref(), &self.get_sign_data())
    }

    /// get all the data relevant to signing contained in this value (excludes signature fields)
    fn get_sign_data(&self) -> Vec<u8> {
        let mut data = serialize(&self.wallclock()).expect("serialize wallclock");

        match self {
            CrdsValue::ContactInfo(contact_info) => contact_info.get_sign_data(),
            CrdsValue::Vote(vote) => {
                let transaction = serialize(&vote.transaction).expect("serialize transaction");
                data.extend_from_slice(&transaction);
                let height = serialize(&vote.transaction).expect("serialize height");
                data.extend_from_slice(&height);
                data
            }
            CrdsValue::LeaderId(leader_id) => {
                let id = serialize(&leader_id.id).expect("serialize id");
                data.extend_from_slice(&id);
                let leader_id = serialize(&leader_id.leader_id).expect("serialize leader_id");
                data.extend_from_slice(&leader_id);
                data
            }
        }
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use contact_info::ContactInfo;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::timing::timestamp;
    use system_transaction::test_tx;

    #[test]
    fn test_labels() {
        let mut hits = [false; 3];
        // this method should cover all the possible labels
        for v in &CrdsValue::record_labels(Pubkey::default()) {
            match v {
                CrdsValueLabel::ContactInfo(_) => hits[0] = true,
                CrdsValueLabel::Vote(_) => hits[1] = true,
                CrdsValueLabel::LeaderId(_) => hits[2] = true,
            }
        }
        assert!(hits.iter().all(|x| *x));
    }
    #[test]
    fn test_keys_and_values() {
        let v = CrdsValue::LeaderId(LeaderId::default());
        let key = v.clone().leader_id().unwrap().id;
        assert_eq!(v.wallclock(), 0);
        assert_eq!(v.label(), CrdsValueLabel::LeaderId(key));

        let v = CrdsValue::ContactInfo(ContactInfo::default());
        assert_eq!(v.wallclock(), 0);
        let key = v.clone().contact_info().unwrap().id;
        assert_eq!(v.label(), CrdsValueLabel::ContactInfo(key));

        let v = CrdsValue::Vote(Vote::new(test_tx(), 1, 0));
        assert_eq!(v.wallclock(), 0);
        let key = v.clone().vote().unwrap().transaction.account_keys[0];
        assert_eq!(v.label(), CrdsValueLabel::Vote(key));
    }
    #[test]
    fn test_signature() {
        let keypair = Keypair::new();
        let fake_keypair = Keypair::new();
        let leader = LeaderId::new(keypair.pubkey(), Pubkey::default(), timestamp());
        let mut v = CrdsValue::LeaderId(leader);
        v.sign(&keypair);
        assert!(v.verify_signature());
        v.sign(&fake_keypair);
        assert!(!v.verify_signature());
    }

}
