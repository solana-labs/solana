use contact_info::ContactInfo;
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
    pub leader_id: Pubkey,
    pub wallclock: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Vote {
    pub transaction: Transaction,
    pub height: u64,
    pub wallclock: u64,
}

/// Type of the replicated value
/// These are labels for values in a record that is assosciated with `Pubkey`
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

impl CrdsValue {
    /// Totally unsecure unverfiable wallclock of the node that generatd this message
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
}
#[cfg(test)]
mod test {
    use super::*;
    use contact_info::ContactInfo;
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

        let v = CrdsValue::Vote(Vote {
            transaction: test_tx(),
            height: 1,
            wallclock: 0,
        });
        assert_eq!(v.wallclock(), 0);
        let key = v.clone().vote().unwrap().transaction.account_keys[0];
        assert_eq!(v.label(), CrdsValueLabel::Vote(key));
    }

}
