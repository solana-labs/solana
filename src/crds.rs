//! This module implements Cluster Replicated Data Store for
//! asynchronous updates in a distributed network.
//!
//! Data is stored in the CrdsValue type, each type has a specific
//! CrdsValueLabel.  Labels are semantically grouped into a single record
//! that is identified by a Pubkey.
//! * 1 Pubkey maps many CrdsValueLabels
//! * 1 CrdsValueLabel maps to 1 CrdsValue
//! The Label, the record Pubkey, and all the record labels can be derived
//! from a single CrdsValue.
//!
//! The actual data is stored in a single map of
//! `CrdsValueLabel(Pubkey) -> CrdsValue` This allows for partial record
//! updates to be propagated through the network.
//!
//! This means that full `Record` updates are not atomic.
//!
//! Additional labels can be added by appending them to the CrdsValueLabel,
//! CrdsValue enums.
//!
//! Merge strategy is implemented in:
//!     impl PartialOrd for VersionedCrdsValue
//!
//! A value is updated to a new version if the labels match, and the value
//! wallclock is later, or the value hash is greater.

use bincode::serialize;
use crds_value::{CrdsValue, CrdsValueLabel};
use indexmap::map::IndexMap;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use std::cmp;

pub struct Crds {
    /// Stores the map of labels and values
    pub table: IndexMap<CrdsValueLabel, VersionedCrdsValue>,
}

#[derive(PartialEq, Debug)]
pub enum CrdsError {
    InsertFailed,
}

/// This structure stores some local metadata associated with the CrdsValue
/// The implementation of PartialOrd ensures that the "highest" version is always picked to be
/// stored in the Crds
#[derive(PartialEq, Debug)]
pub struct VersionedCrdsValue {
    pub value: CrdsValue,
    /// local time when inserted
    pub insert_timestamp: u64,
    /// local time when updated
    pub local_timestamp: u64,
    /// value hash
    pub value_hash: Hash,
}

impl PartialOrd for VersionedCrdsValue {
    fn partial_cmp(&self, other: &VersionedCrdsValue) -> Option<cmp::Ordering> {
        if self.value.label() != other.value.label() {
            None
        } else if self.value.wallclock() == other.value.wallclock() {
            Some(self.value_hash.cmp(&other.value_hash))
        } else {
            Some(self.value.wallclock().cmp(&other.value.wallclock()))
        }
    }
}
impl VersionedCrdsValue {
    pub fn new(local_timestamp: u64, value: CrdsValue) -> Self {
        let value_hash = hash(&serialize(&value).unwrap());
        VersionedCrdsValue {
            value,
            insert_timestamp: local_timestamp,
            local_timestamp,
            value_hash,
        }
    }
}

impl Default for Crds {
    fn default() -> Self {
        Crds {
            table: IndexMap::new(),
        }
    }
}

impl Crds {
    /// must be called atomically with `insert_versioned`
    pub fn new_versioned(&self, local_timestamp: u64, value: CrdsValue) -> VersionedCrdsValue {
        VersionedCrdsValue::new(local_timestamp, value)
    }
    /// insert the new value, returns the old value if insert succeeds
    pub fn insert_versioned(
        &mut self,
        new_value: VersionedCrdsValue,
    ) -> Result<Option<VersionedCrdsValue>, CrdsError> {
        let label = new_value.value.label();
        let wallclock = new_value.value.wallclock();
        let do_insert = self
            .table
            .get(&label)
            .map(|current| new_value > *current)
            .unwrap_or(true);
        if do_insert {
            let old = self.table.insert(label, new_value);
            Ok(old)
        } else {
            trace!("INSERT FAILED data: {} new.wallclock: {}", label, wallclock,);
            Err(CrdsError::InsertFailed)
        }
    }
    pub fn insert(
        &mut self,
        value: CrdsValue,
        local_timestamp: u64,
    ) -> Result<Option<VersionedCrdsValue>, CrdsError> {
        let new_value = self.new_versioned(local_timestamp, value);
        self.insert_versioned(new_value)
    }
    pub fn lookup(&self, label: &CrdsValueLabel) -> Option<&CrdsValue> {
        self.table.get(label).map(|x| &x.value)
    }

    pub fn lookup_versioned(&self, label: &CrdsValueLabel) -> Option<&VersionedCrdsValue> {
        self.table.get(label)
    }

    fn update_label_timestamp(&mut self, id: &CrdsValueLabel, now: u64) {
        if let Some(e) = self.table.get_mut(id) {
            e.local_timestamp = cmp::max(e.local_timestamp, now);
        }
    }

    /// Update the timestamp's of all the labels that are assosciated with Pubkey
    pub fn update_record_timestamp(&mut self, pubkey: Pubkey, now: u64) {
        for label in &CrdsValue::record_labels(pubkey) {
            self.update_label_timestamp(label, now);
        }
    }

    /// find all the keys that are older or equal to min_ts
    pub fn find_old_labels(&self, min_ts: u64) -> Vec<CrdsValueLabel> {
        self.table
            .iter()
            .filter_map(|(k, v)| {
                if v.local_timestamp <= min_ts {
                    Some(k)
                } else {
                    None
                }
            }).cloned()
            .collect()
    }

    pub fn remove(&mut self, key: &CrdsValueLabel) {
        self.table.remove(key);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use contact_info::ContactInfo;
    use crds_value::LeaderId;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_insert() {
        let mut crds = Crds::default();
        let val = CrdsValue::LeaderId(LeaderId::default());
        assert_eq!(crds.insert(val.clone(), 0).ok(), Some(None));
        assert_eq!(crds.table.len(), 1);
        assert!(crds.table.contains_key(&val.label()));
        assert_eq!(crds.table[&val.label()].local_timestamp, 0);
    }
    #[test]
    fn test_update_old() {
        let mut crds = Crds::default();
        let val = CrdsValue::LeaderId(LeaderId::default());
        assert_eq!(crds.insert(val.clone(), 0), Ok(None));
        assert_eq!(crds.insert(val.clone(), 1), Err(CrdsError::InsertFailed));
        assert_eq!(crds.table[&val.label()].local_timestamp, 0);
    }
    #[test]
    fn test_update_new() {
        let mut crds = Crds::default();
        let original = CrdsValue::LeaderId(LeaderId::default());
        assert_matches!(crds.insert(original.clone(), 0), Ok(_));
        let val = CrdsValue::LeaderId(LeaderId::new(Pubkey::default(), Pubkey::default(), 1));
        assert_eq!(
            crds.insert(val.clone(), 1).unwrap().unwrap().value,
            original
        );
        assert_eq!(crds.table[&val.label()].local_timestamp, 1);
    }
    #[test]
    fn test_update_timestsamp() {
        let mut crds = Crds::default();
        let val = CrdsValue::LeaderId(LeaderId::default());
        assert_eq!(crds.insert(val.clone(), 0), Ok(None));

        crds.update_label_timestamp(&val.label(), 1);
        assert_eq!(crds.table[&val.label()].local_timestamp, 1);
        assert_eq!(crds.table[&val.label()].insert_timestamp, 0);

        let val2 = CrdsValue::ContactInfo(ContactInfo::default());
        assert_eq!(val2.label().pubkey(), val.label().pubkey());
        assert_matches!(crds.insert(val2.clone(), 0), Ok(None));

        crds.update_record_timestamp(val.label().pubkey(), 2);
        assert_eq!(crds.table[&val.label()].local_timestamp, 2);
        assert_eq!(crds.table[&val.label()].insert_timestamp, 0);
        assert_eq!(crds.table[&val2.label()].local_timestamp, 2);
        assert_eq!(crds.table[&val2.label()].insert_timestamp, 0);

        crds.update_record_timestamp(val.label().pubkey(), 1);
        assert_eq!(crds.table[&val.label()].local_timestamp, 2);
        assert_eq!(crds.table[&val.label()].insert_timestamp, 0);

        let mut ci = ContactInfo::default();
        ci.wallclock += 1;
        let val3 = CrdsValue::ContactInfo(ci);
        assert_matches!(crds.insert(val3.clone(), 3), Ok(Some(_)));
        assert_eq!(crds.table[&val2.label()].local_timestamp, 3);
        assert_eq!(crds.table[&val2.label()].insert_timestamp, 3);
    }
    #[test]
    fn test_find_old_records() {
        let mut crds = Crds::default();
        let val = CrdsValue::LeaderId(LeaderId::default());
        assert_eq!(crds.insert(val.clone(), 1), Ok(None));

        assert!(crds.find_old_labels(0).is_empty());
        assert_eq!(crds.find_old_labels(1), vec![val.label()]);
        assert_eq!(crds.find_old_labels(2), vec![val.label()]);
    }
    #[test]
    fn test_remove() {
        let mut crds = Crds::default();
        let val = CrdsValue::LeaderId(LeaderId::default());
        assert_matches!(crds.insert(val.clone(), 1), Ok(_));

        assert_eq!(crds.find_old_labels(1), vec![val.label()]);
        crds.remove(&val.label());
        assert!(crds.find_old_labels(1).is_empty());
    }
    #[test]
    fn test_equal() {
        let key = Keypair::new();
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(key.pubkey(), Pubkey::default(), 0)),
        );
        let v2 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(key.pubkey(), Pubkey::default(), 0)),
        );
        assert!(!(v1 != v2));
        assert!(v1 == v2);
    }
    #[test]
    fn test_hash_order() {
        let key = Keypair::new();
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(key.pubkey(), Pubkey::default(), 0)),
        );
        let v2 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(key.pubkey(), key.pubkey(), 0)),
        );
        assert!(v1 != v2);
        assert!(!(v1 == v2));
        if v1 > v2 {
            assert!(v2 < v1)
        } else {
            assert!(v2 > v1)
        }
    }
    #[test]
    fn test_wallclock_order() {
        let key = Keypair::new();
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(key.pubkey(), Pubkey::default(), 1)),
        );
        let v2 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(key.pubkey(), Pubkey::default(), 0)),
        );
        assert!(v1 > v2);
        assert!(!(v1 < v2));
        assert!(v1 != v2);
        assert!(!(v1 == v2));
    }
    #[test]
    fn test_label_order() {
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(Keypair::new().pubkey(), Pubkey::default(), 0)),
        );
        let v2 = VersionedCrdsValue::new(
            1,
            CrdsValue::LeaderId(LeaderId::new(Keypair::new().pubkey(), Pubkey::default(), 0)),
        );
        assert!(v1 != v2);
        assert!(!(v1 == v2));
        assert!(!(v1 < v2));
        assert!(!(v1 > v2));
        assert!(!(v2 < v1));
        assert!(!(v2 > v1));
    }
}
