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

use crate::contact_info::ContactInfo;
use crate::crds_shards::CrdsShards;
use crate::crds_value::{CrdsData, CrdsValue, CrdsValueLabel, LowestSlot};
use bincode::serialize;
use indexmap::map::{rayon::ParValues, Entry, IndexMap};
use indexmap::set::IndexSet;
use rayon::{prelude::*, ThreadPool};
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use std::{
    cmp::Ordering,
    collections::{hash_map, BTreeSet, HashMap},
    ops::{Bound, Index, IndexMut},
};

const CRDS_SHARDS_BITS: u32 = 8;
// Limit number of crds values associated with each unique pubkey. This
// excludes crds values which by label design are limited per each pubkey.
const MAX_CRDS_VALUES_PER_PUBKEY: usize = 32;

#[derive(Clone)]
pub struct Crds {
    /// Stores the map of labels and values
    table: IndexMap<CrdsValueLabel, VersionedCrdsValue>,
    pub num_inserts: usize, // Only used in tests.
    shards: CrdsShards,
    nodes: IndexSet<usize>, // Indices of nodes' ContactInfo.
    votes: IndexSet<usize>, // Indices of Vote crds values.
    // Indices of EpochSlots crds values ordered by insert timestamp.
    epoch_slots: BTreeSet<(u64 /*insert timestamp*/, usize)>,
    // Indices of all crds values associated with a node.
    records: HashMap<Pubkey, IndexSet<usize>>,
}

#[derive(PartialEq, Debug)]
pub enum CrdsError {
    // Hash of the crds value which failed to insert should be recorded in
    // failed_inserts to be excluded from the next pull-request.
    InsertFailed(Hash),
    UnknownStakes,
}

/// This structure stores some local metadata associated with the CrdsValue
/// The implementation of PartialOrd ensures that the "highest" version is always picked to be
/// stored in the Crds
#[derive(PartialEq, Debug, Clone)]
pub struct VersionedCrdsValue {
    pub value: CrdsValue,
    /// local time when inserted
    pub(crate) insert_timestamp: u64,
    /// local time when updated
    pub(crate) local_timestamp: u64,
    /// value hash
    pub(crate) value_hash: Hash,
}

impl VersionedCrdsValue {
    fn new(local_timestamp: u64, value: CrdsValue) -> Self {
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
            table: IndexMap::default(),
            num_inserts: 0,
            shards: CrdsShards::new(CRDS_SHARDS_BITS),
            nodes: IndexSet::default(),
            votes: IndexSet::default(),
            epoch_slots: BTreeSet::default(),
            records: HashMap::default(),
        }
    }
}

// Returns true if the first value updates the 2nd one.
// Both values should have the same key/label.
fn overrides(value: &CrdsValue, other: &VersionedCrdsValue) -> bool {
    assert_eq!(value.label(), other.value.label(), "labels mismatch!");
    match value.wallclock().cmp(&other.value.wallclock()) {
        Ordering::Less => false,
        Ordering::Greater => true,
        // Ties should be broken in a deterministic way across the cluster.
        // For backward compatibility this is done by comparing hash of
        // serialized values.
        Ordering::Equal => {
            let value_hash = hash(&serialize(&value).unwrap());
            other.value_hash < value_hash
        }
    }
}

impl Crds {
    /// Returns true if the given value updates an existing one in the table.
    /// The value is outdated and fails to insert, if it already exists in the
    /// table with a more recent wallclock.
    pub(crate) fn upserts(&self, value: &CrdsValue) -> bool {
        match self.table.get(&value.label()) {
            Some(other) => overrides(value, other),
            None => true,
        }
    }

    pub fn insert(
        &mut self,
        value: CrdsValue,
        local_timestamp: u64,
    ) -> Result<Option<VersionedCrdsValue>, CrdsError> {
        let label = value.label();
        let value = VersionedCrdsValue::new(local_timestamp, value);
        match self.table.entry(label) {
            Entry::Vacant(entry) => {
                let entry_index = entry.index();
                self.shards.insert(entry_index, &value);
                match value.value.data {
                    CrdsData::ContactInfo(_) => {
                        self.nodes.insert(entry_index);
                    }
                    CrdsData::Vote(_, _) => {
                        self.votes.insert(entry_index);
                    }
                    CrdsData::EpochSlots(_, _) => {
                        self.epoch_slots
                            .insert((value.insert_timestamp, entry_index));
                    }
                    _ => (),
                };
                self.records
                    .entry(value.value.pubkey())
                    .or_default()
                    .insert(entry_index);
                entry.insert(value);
                self.num_inserts += 1;
                Ok(None)
            }
            Entry::Occupied(mut entry) if overrides(&value.value, entry.get()) => {
                let entry_index = entry.index();
                self.shards.remove(entry_index, entry.get());
                self.shards.insert(entry_index, &value);
                if let CrdsData::EpochSlots(_, _) = value.value.data {
                    self.epoch_slots
                        .remove(&(entry.get().insert_timestamp, entry_index));
                    self.epoch_slots
                        .insert((value.insert_timestamp, entry_index));
                }
                self.num_inserts += 1;
                // As long as the pubkey does not change, self.records
                // does not need to be updated.
                debug_assert_eq!(entry.get().value.pubkey(), value.value.pubkey());
                Ok(Some(entry.insert(value)))
            }
            _ => {
                trace!(
                    "INSERT FAILED data: {} new.wallclock: {}",
                    value.value.label(),
                    value.value.wallclock(),
                );
                Err(CrdsError::InsertFailed(value.value_hash))
            }
        }
    }

    pub fn lookup(&self, label: &CrdsValueLabel) -> Option<&CrdsValue> {
        self.table.get(label).map(|x| &x.value)
    }

    pub fn lookup_versioned(&self, label: &CrdsValueLabel) -> Option<&VersionedCrdsValue> {
        self.table.get(label)
    }

    pub fn get(&self, label: &CrdsValueLabel) -> Option<&VersionedCrdsValue> {
        self.table.get(label)
    }

    pub fn get_contact_info(&self, pubkey: Pubkey) -> Option<&ContactInfo> {
        let label = CrdsValueLabel::ContactInfo(pubkey);
        self.table.get(&label)?.value.contact_info()
    }

    pub fn get_lowest_slot(&self, pubkey: Pubkey) -> Option<&LowestSlot> {
        let lable = CrdsValueLabel::LowestSlot(pubkey);
        self.table.get(&lable)?.value.lowest_slot()
    }

    /// Returns all entries which are ContactInfo.
    pub fn get_nodes(&self) -> impl Iterator<Item = &VersionedCrdsValue> {
        self.nodes.iter().map(move |i| self.table.index(*i))
    }

    /// Returns ContactInfo of all known nodes.
    pub fn get_nodes_contact_info(&self) -> impl Iterator<Item = &ContactInfo> {
        self.get_nodes().map(|v| match &v.value.data {
            CrdsData::ContactInfo(info) => info,
            _ => panic!("this should not happen!"),
        })
    }

    /// Returns all entries which are Vote.
    pub(crate) fn get_votes(&self) -> impl Iterator<Item = &VersionedCrdsValue> {
        self.votes.iter().map(move |i| self.table.index(*i))
    }

    /// Returns epoch-slots inserted since (or at) the given timestamp.
    pub(crate) fn get_epoch_slots_since(
        &self,
        timestamp: u64,
    ) -> impl Iterator<Item = &VersionedCrdsValue> {
        let range = (Bound::Included((timestamp, 0)), Bound::Unbounded);
        self.epoch_slots
            .range(range)
            .map(move |(_, i)| self.table.index(*i))
    }

    /// Returns all records associated with a pubkey.
    pub(crate) fn get_records(&self, pubkey: &Pubkey) -> impl Iterator<Item = &VersionedCrdsValue> {
        self.records
            .get(pubkey)
            .into_iter()
            .flat_map(|records| records.into_iter())
            .map(move |i| self.table.index(*i))
    }

    /// Returns number of known pubkeys (network size).
    pub(crate) fn num_nodes(&self) -> usize {
        self.records.len()
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn values(&self) -> impl Iterator<Item = &VersionedCrdsValue> {
        self.table.values()
    }

    pub fn par_values(&self) -> ParValues<'_, CrdsValueLabel, VersionedCrdsValue> {
        self.table.par_values()
    }

    /// Returns all crds values which the first 'mask_bits'
    /// of their hash value is equal to 'mask'.
    pub fn filter_bitmask(
        &self,
        mask: u64,
        mask_bits: u32,
    ) -> impl Iterator<Item = &VersionedCrdsValue> {
        self.shards
            .find(mask, mask_bits)
            .map(move |i| self.table.index(i))
    }

    /// Update the timestamp's of all the labels that are associated with Pubkey
    pub fn update_record_timestamp(&mut self, pubkey: &Pubkey, now: u64) {
        // It suffices to only overwrite the origin's timestamp since that is
        // used when purging old values. If the origin does not exist in the
        // table, fallback to exhaustive update on all associated records.
        let origin = CrdsValueLabel::ContactInfo(*pubkey);
        if let Some(origin) = self.table.get_mut(&origin) {
            if origin.local_timestamp < now {
                origin.local_timestamp = now;
            }
        } else if let Some(indices) = self.records.get(pubkey) {
            for index in indices {
                let entry = self.table.index_mut(*index);
                if entry.local_timestamp < now {
                    entry.local_timestamp = now;
                }
            }
        }
    }

    /// Find all the keys that are older or equal to the timeout.
    /// * timeouts - Pubkey specific timeouts with Pubkey::default() as the default timeout.
    pub fn find_old_labels(
        &self,
        thread_pool: &ThreadPool,
        now: u64,
        timeouts: &HashMap<Pubkey, u64>,
    ) -> Vec<CrdsValueLabel> {
        let default_timeout = *timeouts
            .get(&Pubkey::default())
            .expect("must have default timeout");
        // Given an index of all crd values associated with a pubkey,
        // returns crds labels of old values to be evicted.
        let evict = |pubkey, index: &IndexSet<usize>| {
            let timeout = timeouts.get(pubkey).copied().unwrap_or(default_timeout);
            let local_timestamp = {
                let origin = CrdsValueLabel::ContactInfo(*pubkey);
                match self.table.get(&origin) {
                    Some(origin) => origin.local_timestamp,
                    None => 0,
                }
            };
            let mut old_labels = Vec::new();
            // Buffer of crds values to be evicted based on their wallclock.
            let mut recent_unlimited_labels: Vec<(u64 /*wallclock*/, usize /*index*/)> = index
                .into_iter()
                .filter_map(|ix| {
                    let (label, value) = self.table.get_index(*ix).unwrap();
                    let expiry_timestamp = value
                        .local_timestamp
                        .max(local_timestamp)
                        .saturating_add(timeout);
                    if expiry_timestamp <= now {
                        old_labels.push(label.clone());
                        None
                    } else {
                        match label.value_space() {
                            Some(_) => None,
                            None => Some((value.value.wallclock(), *ix)),
                        }
                    }
                })
                .collect();
            // Number of values to discard from the buffer:
            let nth = recent_unlimited_labels
                .len()
                .saturating_sub(MAX_CRDS_VALUES_PER_PUBKEY);
            // Partition on wallclock to discard the older ones.
            if nth > 0 && nth < recent_unlimited_labels.len() {
                recent_unlimited_labels.select_nth_unstable(nth);
            }
            old_labels.extend(
                recent_unlimited_labels
                    .split_at(nth)
                    .0
                    .iter()
                    .map(|(_ /*wallclock*/, ix)| self.table.get_index(*ix).unwrap().0.clone()),
            );
            old_labels
        };
        thread_pool.install(|| {
            self.records
                .par_iter()
                .flat_map(|(pubkey, index)| evict(pubkey, index))
                .collect()
        })
    }

    pub fn remove(&mut self, key: &CrdsValueLabel) -> Option<VersionedCrdsValue> {
        let (index, _ /*label*/, value) = self.table.swap_remove_full(key)?;
        self.shards.remove(index, &value);
        match value.value.data {
            CrdsData::ContactInfo(_) => {
                self.nodes.swap_remove(&index);
            }
            CrdsData::Vote(_, _) => {
                self.votes.swap_remove(&index);
            }
            CrdsData::EpochSlots(_, _) => {
                self.epoch_slots.remove(&(value.insert_timestamp, index));
            }
            _ => (),
        }
        // Remove the index from records associated with the value's pubkey.
        let pubkey = value.value.pubkey();
        let mut records_entry = match self.records.entry(pubkey) {
            hash_map::Entry::Vacant(_) => panic!("this should not happen!"),
            hash_map::Entry::Occupied(entry) => entry,
        };
        records_entry.get_mut().swap_remove(&index);
        if records_entry.get().is_empty() {
            records_entry.remove();
        }
        // If index == self.table.len(), then the removed entry was the last
        // entry in the table, in which case no other keys were modified.
        // Otherwise, the previously last element in the table is now moved to
        // the 'index' position; and so shards and nodes need to be updated
        // accordingly.
        let size = self.table.len();
        if index < size {
            let value = self.table.index(index);
            self.shards.remove(size, value);
            self.shards.insert(index, value);
            match value.value.data {
                CrdsData::ContactInfo(_) => {
                    self.nodes.swap_remove(&size);
                    self.nodes.insert(index);
                }
                CrdsData::Vote(_, _) => {
                    self.votes.swap_remove(&size);
                    self.votes.insert(index);
                }
                CrdsData::EpochSlots(_, _) => {
                    self.epoch_slots.remove(&(value.insert_timestamp, size));
                    self.epoch_slots.insert((value.insert_timestamp, index));
                }
                _ => (),
            };
            let pubkey = value.value.pubkey();
            let records = self.records.get_mut(&pubkey).unwrap();
            records.swap_remove(&size);
            records.insert(index);
        }
        Some(value)
    }

    /// Returns true if the number of unique pubkeys in the table exceeds the
    /// given capacity (plus some margin).
    /// Allows skipping unnecessary calls to trim without obtaining a write
    /// lock on gossip.
    pub(crate) fn should_trim(&self, cap: usize) -> bool {
        // Allow 10% overshoot so that the computation cost is amortized down.
        10 * self.records.len() > 11 * cap
    }

    /// Trims the table by dropping all values associated with the pubkeys with
    /// the lowest stake, so that the number of unique pubkeys are bounded.
    pub(crate) fn trim(
        &mut self,
        cap: usize, // Capacity hint for number of unique pubkeys.
        // Set of pubkeys to never drop.
        // e.g. trusted validators, self pubkey, ...
        keep: &[Pubkey],
        stakes: &HashMap<Pubkey, u64>,
    ) -> Result<Vec<VersionedCrdsValue>, CrdsError> {
        if self.should_trim(cap) {
            let size = self.records.len().saturating_sub(cap);
            self.drop(size, keep, stakes)
        } else {
            Ok(Vec::default())
        }
    }

    // Drops 'size' many pubkeys with the lowest stake.
    fn drop(
        &mut self,
        size: usize,
        keep: &[Pubkey],
        stakes: &HashMap<Pubkey, u64>,
    ) -> Result<Vec<VersionedCrdsValue>, CrdsError> {
        if stakes.is_empty() {
            return Err(CrdsError::UnknownStakes);
        }
        let mut keys: Vec<_> = self
            .records
            .keys()
            .map(|k| (stakes.get(k).copied().unwrap_or_default(), *k))
            .collect();
        if size < keys.len() {
            keys.select_nth_unstable(size);
        }
        let keys: Vec<_> = keys
            .into_iter()
            .take(size)
            .map(|(_, k)| k)
            .filter(|k| !keep.contains(k))
            .flat_map(|k| &self.records[&k])
            .map(|k| self.table.get_index(*k).unwrap().0.clone())
            .collect();
        Ok(keys.iter().map(|k| self.remove(k).unwrap()).collect())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        contact_info::ContactInfo,
        crds_value::{new_rand_timestamp, NodeInstance},
    };
    use rand::{thread_rng, Rng};
    use rayon::ThreadPoolBuilder;
    use solana_sdk::signature::{Keypair, Signer};
    use std::{collections::HashSet, iter::repeat_with};

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
        let value_hash = hash(&serialize(&val).unwrap());
        assert_eq!(crds.insert(val.clone(), 0), Ok(None));
        assert_eq!(
            crds.insert(val.clone(), 1),
            Err(CrdsError::InsertFailed(value_hash))
        );
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
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(crds.insert(val.clone(), 1), Ok(None));
        let mut set = HashMap::new();
        set.insert(Pubkey::default(), 0);
        assert!(crds.find_old_labels(&thread_pool, 0, &set).is_empty());
        set.insert(Pubkey::default(), 1);
        assert_eq!(
            crds.find_old_labels(&thread_pool, 2, &set),
            vec![val.label()]
        );
        set.insert(Pubkey::default(), 2);
        assert_eq!(
            crds.find_old_labels(&thread_pool, 4, &set),
            vec![val.label()]
        );
    }
    #[test]
    fn test_find_old_records_with_override() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut rng = thread_rng();
        let mut crds = Crds::default();
        let mut timeouts = HashMap::new();
        let val = CrdsValue::new_rand(&mut rng, None);
        timeouts.insert(Pubkey::default(), 3);
        assert_eq!(crds.insert(val.clone(), 0), Ok(None));
        assert!(crds.find_old_labels(&thread_pool, 2, &timeouts).is_empty());
        timeouts.insert(val.pubkey(), 1);
        assert_eq!(
            crds.find_old_labels(&thread_pool, 2, &timeouts),
            vec![val.label()]
        );
        timeouts.insert(val.pubkey(), u64::MAX);
        assert!(crds.find_old_labels(&thread_pool, 2, &timeouts).is_empty());
        timeouts.insert(Pubkey::default(), 1);
        assert!(crds.find_old_labels(&thread_pool, 2, &timeouts).is_empty());
        timeouts.remove(&val.pubkey());
        assert_eq!(
            crds.find_old_labels(&thread_pool, 2, &timeouts),
            vec![val.label()]
        );
    }

    #[test]
    fn test_find_old_records_unlimited() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut rng = thread_rng();
        let now = 1_610_034_423_000;
        let pubkey = Pubkey::new_unique();
        let mut crds = Crds::default();
        let mut timeouts = HashMap::new();
        timeouts.insert(Pubkey::default(), 1);
        timeouts.insert(pubkey, 180);
        for _ in 0..1024 {
            let wallclock = now - rng.gen_range(0, 240);
            let val = NodeInstance::new(&mut rng, pubkey, wallclock);
            let val = CrdsData::NodeInstance(val);
            let val = CrdsValue::new_unsigned(val);
            assert_eq!(crds.insert(val, now), Ok(None));
        }
        let now = now + 1;
        let labels = crds.find_old_labels(&thread_pool, now, &timeouts);
        assert_eq!(crds.table.len() - labels.len(), MAX_CRDS_VALUES_PER_PUBKEY);
        let max_wallclock = labels
            .iter()
            .map(|label| crds.lookup(label).unwrap().wallclock())
            .max()
            .unwrap();
        assert!(max_wallclock > now - 180);
        let labels: HashSet<_> = labels.into_iter().collect();
        for (label, value) in crds.table.iter() {
            if !labels.contains(label) {
                assert!(max_wallclock <= value.value.wallclock());
            }
        }
    }

    #[test]
    fn test_remove_default() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_matches!(crds.insert(val.clone(), 1), Ok(_));
        let mut set = HashMap::new();
        set.insert(Pubkey::default(), 1);
        assert_eq!(
            crds.find_old_labels(&thread_pool, 2, &set),
            vec![val.label()]
        );
        crds.remove(&val.label());
        assert!(crds.find_old_labels(&thread_pool, 2, &set).is_empty());
    }
    #[test]
    fn test_find_old_records_staked() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_eq!(crds.insert(val.clone(), 1), Ok(None));
        let mut set = HashMap::new();
        //now < timestamp
        set.insert(Pubkey::default(), 0);
        set.insert(val.pubkey(), 0);
        assert!(crds.find_old_labels(&thread_pool, 0, &set).is_empty());

        //pubkey shouldn't expire since its timeout is MAX
        set.insert(val.pubkey(), std::u64::MAX);
        assert!(crds.find_old_labels(&thread_pool, 2, &set).is_empty());

        //default has max timeout, but pubkey should still expire
        set.insert(Pubkey::default(), std::u64::MAX);
        set.insert(val.pubkey(), 1);
        assert_eq!(
            crds.find_old_labels(&thread_pool, 2, &set),
            vec![val.label()]
        );

        set.insert(val.pubkey(), 2);
        assert!(crds.find_old_labels(&thread_pool, 2, &set).is_empty());
        assert_eq!(
            crds.find_old_labels(&thread_pool, 3, &set),
            vec![val.label()]
        );
    }

    #[test]
    fn test_crds_shards() {
        fn check_crds_shards(crds: &Crds) {
            crds.shards
                .check(&crds.table.values().cloned().collect::<Vec<_>>());
        }

        let mut crds = Crds::default();
        let keypairs: Vec<_> = std::iter::repeat_with(Keypair::new).take(256).collect();
        let mut rng = thread_rng();
        let mut num_inserts = 0;
        let mut num_overrides = 0;
        for _ in 0..4096 {
            let keypair = &keypairs[rng.gen_range(0, keypairs.len())];
            let value = CrdsValue::new_rand(&mut rng, Some(keypair));
            let local_timestamp = new_rand_timestamp(&mut rng);
            match crds.insert(value, local_timestamp) {
                Ok(None) => {
                    num_inserts += 1;
                    check_crds_shards(&crds);
                }
                Ok(Some(_)) => {
                    num_inserts += 1;
                    num_overrides += 1;
                    check_crds_shards(&crds);
                }
                Err(_) => (),
            }
        }
        assert_eq!(num_inserts, crds.num_inserts);
        assert!(num_inserts > 700);
        assert!(num_overrides > 500);
        assert!(crds.table.len() > 200);
        assert!(num_inserts > crds.table.len());
        check_crds_shards(&crds);
        // Remove values one by one and assert that shards stay valid.
        while !crds.table.is_empty() {
            let index = rng.gen_range(0, crds.table.len());
            let key = crds.table.get_index(index).unwrap().0.clone();
            crds.remove(&key);
            check_crds_shards(&crds);
        }
    }

    #[test]
    fn test_crds_value_indices() {
        fn check_crds_value_indices<R: rand::Rng>(
            rng: &mut R,
            crds: &Crds,
        ) -> (usize, usize, usize) {
            if !crds.table.is_empty() {
                let since = crds.table[rng.gen_range(0, crds.table.len())].insert_timestamp;
                let num_epoch_slots = crds
                    .table
                    .values()
                    .filter(|value| {
                        value.insert_timestamp >= since
                            && matches!(value.value.data, CrdsData::EpochSlots(_, _))
                    })
                    .count();
                assert_eq!(num_epoch_slots, crds.get_epoch_slots_since(since).count());
                for value in crds.get_epoch_slots_since(since) {
                    assert!(value.insert_timestamp >= since);
                    match value.value.data {
                        CrdsData::EpochSlots(_, _) => (),
                        _ => panic!("not an epoch-slot!"),
                    }
                }
            }
            let num_nodes = crds
                .table
                .values()
                .filter(|value| matches!(value.value.data, CrdsData::ContactInfo(_)))
                .count();
            let num_votes = crds
                .table
                .values()
                .filter(|value| matches!(value.value.data, CrdsData::Vote(_, _)))
                .count();
            let num_epoch_slots = crds
                .table
                .values()
                .filter(|value| matches!(value.value.data, CrdsData::EpochSlots(_, _)))
                .count();
            assert_eq!(num_nodes, crds.get_nodes_contact_info().count());
            assert_eq!(num_votes, crds.get_votes().count());
            assert_eq!(num_epoch_slots, crds.get_epoch_slots_since(0).count());
            for vote in crds.get_votes() {
                match vote.value.data {
                    CrdsData::Vote(_, _) => (),
                    _ => panic!("not a vote!"),
                }
            }
            for epoch_slots in crds.get_epoch_slots_since(0) {
                match epoch_slots.value.data {
                    CrdsData::EpochSlots(_, _) => (),
                    _ => panic!("not an epoch-slot!"),
                }
            }
            (num_nodes, num_votes, num_epoch_slots)
        }
        let mut rng = thread_rng();
        let keypairs: Vec<_> = repeat_with(Keypair::new).take(128).collect();
        let mut crds = Crds::default();
        let mut num_inserts = 0;
        let mut num_overrides = 0;
        for k in 0..4096 {
            let keypair = &keypairs[rng.gen_range(0, keypairs.len())];
            let value = CrdsValue::new_rand(&mut rng, Some(keypair));
            let local_timestamp = new_rand_timestamp(&mut rng);
            match crds.insert(value, local_timestamp) {
                Ok(None) => {
                    num_inserts += 1;
                }
                Ok(Some(_)) => {
                    num_inserts += 1;
                    num_overrides += 1;
                }
                Err(_) => (),
            }
            if k % 64 == 0 {
                check_crds_value_indices(&mut rng, &crds);
            }
        }
        assert_eq!(num_inserts, crds.num_inserts);
        assert!(num_inserts > 700);
        assert!(num_overrides > 500);
        assert!(crds.table.len() > 200);
        assert!(num_inserts > crds.table.len());
        let (num_nodes, num_votes, num_epoch_slots) = check_crds_value_indices(&mut rng, &crds);
        assert!(num_nodes * 3 < crds.table.len());
        assert!(num_nodes > 100, "num nodes: {}", num_nodes);
        assert!(num_votes > 100, "num votes: {}", num_votes);
        assert!(
            num_epoch_slots > 100,
            "num epoch slots: {}",
            num_epoch_slots
        );
        // Remove values one by one and assert that nodes indices stay valid.
        while !crds.table.is_empty() {
            let index = rng.gen_range(0, crds.table.len());
            let key = crds.table.get_index(index).unwrap().0.clone();
            crds.remove(&key);
            if crds.table.len() % 64 == 0 {
                check_crds_value_indices(&mut rng, &crds);
            }
        }
    }

    #[test]
    fn test_crds_records() {
        fn check_crds_records(crds: &Crds) {
            assert_eq!(
                crds.table.len(),
                crds.records.values().map(IndexSet::len).sum::<usize>()
            );
            for (pubkey, indices) in &crds.records {
                for index in indices {
                    let value = crds.table.index(*index);
                    assert_eq!(*pubkey, value.value.pubkey());
                }
            }
        }
        let mut rng = thread_rng();
        let keypairs: Vec<_> = repeat_with(Keypair::new).take(128).collect();
        let mut crds = Crds::default();
        for k in 0..4096 {
            let keypair = &keypairs[rng.gen_range(0, keypairs.len())];
            let value = CrdsValue::new_rand(&mut rng, Some(keypair));
            let local_timestamp = new_rand_timestamp(&mut rng);
            let _ = crds.insert(value, local_timestamp);
            if k % 64 == 0 {
                check_crds_records(&crds);
            }
        }
        assert!(crds.records.len() > 96);
        assert!(crds.records.len() <= keypairs.len());
        // Remove values one by one and assert that records stay valid.
        while !crds.table.is_empty() {
            let index = rng.gen_range(0, crds.table.len());
            let key = crds.table.get_index(index).unwrap().0.clone();
            crds.remove(&key);
            if crds.table.len() % 64 == 0 {
                check_crds_records(&crds);
            }
        }
        assert!(crds.records.is_empty());
    }

    #[test]
    fn test_drop() {
        fn num_unique_pubkeys<'a, I>(values: I) -> usize
        where
            I: IntoIterator<Item = &'a VersionedCrdsValue>,
        {
            values
                .into_iter()
                .map(|v| v.value.pubkey())
                .collect::<HashSet<_>>()
                .len()
        }
        let mut rng = thread_rng();
        let keypairs: Vec<_> = repeat_with(Keypair::new).take(64).collect();
        let stakes = keypairs
            .iter()
            .map(|k| (k.pubkey(), rng.gen_range(0, 1000)))
            .collect();
        let mut crds = Crds::default();
        for _ in 0..2048 {
            let keypair = &keypairs[rng.gen_range(0, keypairs.len())];
            let value = CrdsValue::new_rand(&mut rng, Some(keypair));
            let local_timestamp = new_rand_timestamp(&mut rng);
            let _ = crds.insert(value, local_timestamp);
        }
        let num_values = crds.table.len();
        let num_pubkeys = num_unique_pubkeys(crds.table.values());
        assert!(!crds.should_trim(num_pubkeys));
        assert!(crds.should_trim(num_pubkeys * 5 / 6));
        let purged = crds.drop(16, &[], &stakes).unwrap();
        assert_eq!(purged.len() + crds.table.len(), num_values);
        assert_eq!(num_unique_pubkeys(&purged), 16);
        assert_eq!(num_unique_pubkeys(crds.table.values()), num_pubkeys - 16);
        let attach_stake = |v: &VersionedCrdsValue| {
            let pk = v.value.pubkey();
            (stakes[&pk], pk)
        };
        assert!(
            purged.iter().map(attach_stake).max().unwrap()
                < crds.table.values().map(attach_stake).min().unwrap()
        );
        let purged = purged
            .into_iter()
            .map(|v| v.value.pubkey())
            .collect::<HashSet<_>>();
        for (k, v) in crds.table {
            assert!(!purged.contains(&k.pubkey()));
            assert!(!purged.contains(&v.value.pubkey()));
        }
    }

    #[test]
    fn test_remove_staked() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let mut crds = Crds::default();
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        assert_matches!(crds.insert(val.clone(), 1), Ok(_));
        let mut set = HashMap::new();

        //default has max timeout, but pubkey should still expire
        set.insert(Pubkey::default(), std::u64::MAX);
        set.insert(val.pubkey(), 1);
        assert_eq!(
            crds.find_old_labels(&thread_pool, 2, &set),
            vec![val.label()]
        );
        crds.remove(&val.label());
        assert!(crds.find_old_labels(&thread_pool, 2, &set).is_empty());
    }

    #[test]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn test_equal() {
        let val = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        let v1 = VersionedCrdsValue::new(1, val.clone());
        let v2 = VersionedCrdsValue::new(1, val);
        assert_eq!(v1, v2);
        assert!(!(v1 != v2));
        assert!(!overrides(&v1.value, &v2));
        assert!(!overrides(&v2.value, &v1));
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
        if v1.value_hash > v2.value_hash {
            assert!(overrides(&v1.value, &v2));
            assert!(!overrides(&v2.value, &v1));
        } else {
            assert!(overrides(&v2.value, &v1));
            assert!(!overrides(&v1.value, &v2));
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
        assert!(overrides(&v1.value, &v2));
        assert!(!overrides(&v2.value, &v1));
        assert!(v1 != v2);
        assert!(!(v1 == v2));
    }
    #[test]
    #[should_panic(expected = "labels mismatch!")]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn test_label_order() {
        let v1 = VersionedCrdsValue::new(
            1,
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &solana_sdk::pubkey::new_rand(),
                0,
            ))),
        );
        let v2 = VersionedCrdsValue::new(
            1,
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::new_localhost(
                &solana_sdk::pubkey::new_rand(),
                0,
            ))),
        );
        assert_ne!(v1, v2);
        assert!(!(v1 == v2));
        assert!(!overrides(&v2.value, &v1));
    }
}
