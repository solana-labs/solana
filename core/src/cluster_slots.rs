use crate::{cluster_info::ClusterInfo, crds_value::EpochSlots, serve_repair::RepairType};

use solana_ledger::{bank_forks::BankForks, staking_utils};
use solana_sdk::{clock::Slot, pubkey::Pubkey};

use std::{
    collections::{BTreeSet, HashMap},
    sync::RwLock,
};

#[derive(Default)]
pub struct ClusterSlots {
    cluster_slots: HashMap<Slot, HashMap<Pubkey, u64>>,
    since: Option<u64>,
}

impl ClusterSlots {
    pub fn lookup(&self, slot: Slot) -> Option<&HashMap<Pubkey, u64>> {
        self.cluster_slots.get(&slot)
    }
    pub fn update(
        &mut self,
        root: Slot,
        cluster_info: &RwLock<ClusterInfo>,
        bank_forks: &RwLock<BankForks>,
    ) {
        let (epoch, _) = bank_forks
            .read()
            .unwrap()
            .working_bank()
            .get_epoch_and_slot_index(root);
        let stakes =
            staking_utils::staked_nodes_at_epoch(&bank_forks.read().unwrap().working_bank(), epoch);
        let epoch_slots = cluster_info
            .read()
            .unwrap()
            .get_epoch_slots_since(self.since);
        self.update_internal(root, epoch_slots, stakes);
    }
    fn update_internal(
        &mut self,
        root: Slot,
        epoch_slots: (Vec<EpochSlots>, Option<u64>),
        stakes: Option<HashMap<Pubkey, u64>>,
    ) {
        let (epoch_slots_list, since) = epoch_slots;
        for epoch_slots in epoch_slots_list {
            for slot in &epoch_slots.slots {
                if *slot < root {
                    continue;
                }
                let balance = stakes
                    .as_ref()
                    .and_then(|s| s.get(&epoch_slots.from))
                    .cloned()
                    .unwrap_or(0);
                self.cluster_slots
                    .entry(*slot)
                    .or_insert_with(HashMap::default)
                    .insert(epoch_slots.from, balance + 1);
            }
        }
        self.cluster_slots.retain(|x, _| *x > root);
        self.since = since;
    }

    pub fn generate_repairs_for_missing_slots(
        &self,
        root: Slot,
        epoch_slots: &BTreeSet<Slot>,
        old_incomplete_slots: &BTreeSet<Slot>,
    ) -> Vec<RepairType> {
        self.cluster_slots
            .keys()
            .filter(|x| **x > root)
            .filter(|x| !(epoch_slots.contains(*x) || old_incomplete_slots.contains(*x)))
            .map(|x| RepairType::Orphan(*x))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let cs = ClusterSlots::default();
        assert!(cs.cluster_slots.is_empty());
        assert!(cs.since.is_none());
    }

    #[test]
    fn test_update_noop() {
        let mut cs = ClusterSlots::default();
        cs.update_internal(0, (vec![], None), None);
        assert!(cs.cluster_slots.is_empty());
        assert!(cs.since.is_none());
    }

    #[test]
    fn test_update_empty() {
        let mut cs = ClusterSlots::default();
        let epoch_slot = EpochSlots::default();
        cs.update_internal(0, (vec![epoch_slot], Some(0)), None);
        assert_eq!(cs.since, Some(0));
        assert!(cs.lookup(0).is_none());
    }

    #[test]
    fn test_update_rooted() {
        //root is 0, so it should clear out the slot
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.slots.insert(0);
        cs.update_internal(0, (vec![epoch_slot], Some(0)), None);
        assert_eq!(cs.since, Some(0));
        assert!(cs.lookup(0).is_none());
    }

    #[test]
    fn test_update_new_slot() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.slots.insert(1);
        cs.update_internal(0, (vec![epoch_slot], Some(0)), None);
        assert_eq!(cs.since, Some(0));
        assert!(cs.lookup(0).is_none());
        assert!(cs.lookup(1).is_some());
        assert_eq!(cs.lookup(1).unwrap().get(&Pubkey::default()), Some(&1));
    }

    #[test]
    fn test_update_new_staked_slot() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.slots.insert(1);
        let map = vec![(Pubkey::default(), 1)].into_iter().collect();
        cs.update_internal(0, (vec![epoch_slot], None), Some(map));
        assert_eq!(cs.lookup(1).unwrap().get(&Pubkey::default()), Some(&2));
    }

    #[test]
    fn test_generate_repairs() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.slots.insert(1);
        cs.update_internal(0, (vec![epoch_slot], None), None);
        assert_eq!(
            cs.generate_repairs_for_missing_slots(0, &BTreeSet::new(), &BTreeSet::new()),
            vec![RepairType::Orphan(1)]
        )
    }

    #[test]
    fn test_generate_repairs_existing() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.slots.insert(1);
        cs.update_internal(0, (vec![epoch_slot], None), None);
        let mut existing = BTreeSet::new();
        existing.insert(1);
        assert_eq!(
            cs.generate_repairs_for_missing_slots(0, &existing, &BTreeSet::new()),
            vec![]
        );
        assert_eq!(
            cs.generate_repairs_for_missing_slots(0, &BTreeSet::new(), &existing),
            vec![]
        );
    }
}
