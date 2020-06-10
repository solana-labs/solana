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

use crate::crds_value::{CrdsValue, CrdsValueLabel};
use bincode::serialize;
use indexmap::map::IndexMap;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use std::cmp;
use std::collections::HashMap;

#[derive(Clone)]
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
#[derive(PartialEq, Debug, Clone)]
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
    pub fn would_insert(
        &self,
        value: CrdsValue,
        local_timestamp: u64,
    ) -> Option<VersionedCrdsValue> {
        let new_value = self.new_versioned(local_timestamp, value);
        let label = new_value.value.label();
        let would_insert = self
            .table
            .get(&label)
            .map(|current| new_value > *current)
            .unwrap_or(true);
        if would_insert {
            Some(new_value)
        } else {
            None
        }
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
    pub fn update_record_timestamp(&mut self, pubkey: &Pubkey, now: u64) {
        for label in &CrdsValue::record_labels(pubkey) {
            self.update_label_timestamp(label, now);
        }
    }

    /// Find all the keys that are older or equal to the timeout.
    /// * timeouts - Pubkey specific timeouts with Pubkey::default() as the default timeout.
    pub fn find_old_labels(
        &self,
        now: u64,
        timeouts: &HashMap<Pubkey, u64>,
    ) -> Vec<CrdsValueLabel> {
        let min_ts = *timeouts
            .get(&Pubkey::default())
            .expect("must have default timeout");
        self.table
            .iter()
            .filter_map(|(k, v)| {
                if now < v.local_timestamp
                    || (timeouts.get(&k.pubkey()).is_some()
                        && now - v.local_timestamp < timeouts[&k.pubkey()])
                {
                    None
                } else if now - v.local_timestamp >= min_ts {
                    Some(k)
                } else {
                    None
                }
            })
            .cloned()
            .collect()
    }

    pub fn remove(&mut self, key: &CrdsValueLabel) {
        self.table.swap_remove(key);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::contact_info::ContactInfo;
    use crate::crds_value::CrdsData;

    #[test]
    fn test_insert() {
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(crds.insert(val.clone(), 0).ok(), Some(None));
        assert_eq!(crds.table.len(), 1);
        assert!(crds.table.contains_key(&val.label()));
        assert_eq!(crds.table[&val.label()].local_timestamp, 0);
    }
    #[test]
    fn test_update_old() {
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(crds.insert(val.clone(), 0), Ok(None));
        assert_eq!(crds.insert(val.clone(), 1), Err(CrdsError::InsertFailed));
        assert_eq!(crds.table[&val.label()].local_timestamp, 0);
    }
    #[test]
    fn test_update_new() {
        let mut crds = Crds::default();
        let original = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::default(),
            0,
        )));
        assert_matches!(crds.insert(original.clone(), 0), Ok(_));
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::default(),
            1,
        )));
        assert_eq!(
            crds.insert(val.clone(), 1).unwrap().unwrap().value,
            original
        );
        assert_eq!(crds.table[&val.label()].local_timestamp, 1);
    }
    #[test]
    fn test_update_timestamp() {
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
            &Pubkey::default(),
            0,
        )));
        assert_eq!(crds.insert(val.clone(), 0), Ok(None));

        crds.update_label_timestamp(&val.label(), 1);
        assert_eq!(crds.table[&val.label()].local_timestamp, 1);
        assert_eq!(crds.table[&val.label()].insert_timestamp, 0);

        let val2 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(val2.label().pubkey(), val.label().pubkey());
        assert_matches!(crds.insert(val2.clone(), 0), Ok(Some(_)));

        crds.update_record_timestamp(&val.label().pubkey(), 2);
        assert_eq!(crds.table[&val.label()].local_timestamp, 2);
        assert_eq!(crds.table[&val.label()].insert_timestamp, 0);
        assert_eq!(crds.table[&val2.label()].local_timestamp, 2);
        assert_eq!(crds.table[&val2.label()].insert_timestamp, 0);

        crds.update_record_timestamp(&val.label().pubkey(), 1);
        assert_eq!(crds.table[&val.label()].local_timestamp, 2);
        assert_eq!(crds.table[&val.label()].insert_timestamp, 0);

        let mut ci = ContactInfo::default();
        ci.wallclock += 1;
        let val3 = CrdsValue::new_unsigned(CrdsData::ContactInfo(ci));
        assert_matches!(crds.insert(val3, 3), Ok(Some(_)));
        assert_eq!(crds.table[&val2.label()].local_timestamp, 3);
        assert_eq!(crds.table[&val2.label()].insert_timestamp, 3);
    }
    #[test]
    fn test_find_old_records_default() {
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(crds.insert(val.clone(), 1), Ok(None));
        let mut set = HashMap::new();
        set.insert(Pubkey::default(), 0);
        assert!(crds.find_old_labels(0, &set).is_empty());
        set.insert(Pubkey::default(), 1);
        assert_eq!(crds.find_old_labels(2, &set), vec![val.label()]);
        set.insert(Pubkey::default(), 2);
        assert_eq!(crds.find_old_labels(4, &set), vec![val.label()]);
    }
    #[test]
    fn test_remove_default() {
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_matches!(crds.insert(val.clone(), 1), Ok(_));
        let mut set = HashMap::new();
        set.insert(Pubkey::default(), 1);
        assert_eq!(crds.find_old_labels(2, &set), vec![val.label()]);
        crds.remove(&val.label());
        assert!(crds.find_old_labels(2, &set).is_empty());
    }
    #[test]
    fn test_find_old_records_staked() {
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(crds.insert(val.clone(), 1), Ok(None));
        let mut set = HashMap::new();
        //now < timestamp
        set.insert(Pubkey::default(), 0);
        set.insert(val.pubkey(), 0);
        assert!(crds.find_old_labels(0, &set).is_empty());

        //pubkey shouldn't expire since its timeout is MAX
        set.insert(val.pubkey(), std::u64::MAX);
        assert!(crds.find_old_labels(2, &set).is_empty());

        //default has max timeout, but pubkey should still expire
        set.insert(Pubkey::default(), std::u64::MAX);
        set.insert(val.pubkey(), 1);
        assert_eq!(crds.find_old_labels(2, &set), vec![val.label()]);

        set.insert(val.pubkey(), 2);
        assert!(crds.find_old_labels(2, &set).is_empty());
        assert_eq!(crds.find_old_labels(3, &set), vec![val.label()]);
    }

    #[test]
    fn test_remove_staked() {
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_matches!(crds.insert(val.clone(), 1), Ok(_));
        let mut set = HashMap::new();

        //default has max timeout, but pubkey should still expire
        set.insert(Pubkey::default(), std::u64::MAX);
        set.insert(val.pubkey(), 1);
        assert_eq!(crds.find_old_labels(2, &set), vec![val.label()]);
        crds.remove(&val.label());
        assert!(crds.find_old_labels(2, &set).is_empty());
    }

    #[test]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn test_equal() {
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        let v1 = VersionedCrdsValue::new(1, val.clone());
        let v2 = VersionedCrdsValue::new(1, val);
        assert_eq!(v1, v2);
        assert!(!(v1 != v2));
        assert_eq!(v1.partial_cmp(&v2), Some(cmp::Ordering::Equal));
        assert_eq!(v2.partial_cmp(&v1), Some(cmp::Ordering::Equal));
    }
    #[test]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn test_hash_order() {
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &Pubkey::default(),
                0,
            ))),
        );
        let v2 = VersionedCrdsValue::new(1, {
            let mut contact_info = ContactInfo::new_localhost(&Pubkey::default(), 0);
            contact_info.rpc = socketaddr!("0.0.0.0:0");
            CrdsValue::new_unsigned(CrdsData::ContactInfo(contact_info))
        });

        assert_eq!(v1.value.label(), v2.value.label());
        assert_eq!(v1.value.wallclock(), v2.value.wallclock());
        assert_ne!(v1.value_hash, v2.value_hash);
        assert!(v1 != v2);
        assert!(!(v1 == v2));
        if v1 > v2 {
            assert!(v1 > v2);
            assert!(v2 < v1);
            assert_eq!(v1.partial_cmp(&v2), Some(cmp::Ordering::Greater));
            assert_eq!(v2.partial_cmp(&v1), Some(cmp::Ordering::Less));
        } else if v2 > v1 {
            assert!(v1 < v2);
            assert!(v2 > v1);
            assert_eq!(v1.partial_cmp(&v2), Some(cmp::Ordering::Less));
            assert_eq!(v2.partial_cmp(&v1), Some(cmp::Ordering::Greater));
        } else {
            panic!("bad PartialOrd implementation?");
        }
    }
    #[test]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn test_wallclock_order() {
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &Pubkey::default(),
                1,
            ))),
        );
        let v2 = VersionedCrdsValue::new(
            1,
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &Pubkey::default(),
                0,
            ))),
        );
        assert_eq!(v1.value.label(), v2.value.label());
        assert!(v1 > v2);
        assert!(!(v1 < v2));
        assert!(v1 != v2);
        assert!(!(v1 == v2));
        assert_eq!(v1.partial_cmp(&v2), Some(cmp::Ordering::Greater));
        assert_eq!(v2.partial_cmp(&v1), Some(cmp::Ordering::Less));
    }
    #[test]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn test_label_order() {
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &Pubkey::new_rand(),
                0,
            ))),
        );
        let v2 = VersionedCrdsValue::new(
            1,
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &Pubkey::new_rand(),
                0,
            ))),
        );
        assert_ne!(v1, v2);
        assert!(!(v1 == v2));
        assert!(!(v1 < v2));
        assert!(!(v1 > v2));
        assert!(!(v2 < v1));
        assert!(!(v2 > v1));
        assert_eq!(v1.partial_cmp(&v2), None);
        assert_eq!(v2.partial_cmp(&v1), None);
    }
}
