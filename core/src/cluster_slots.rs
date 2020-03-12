use crate::{cluster_info::ClusterInfo, epoch_slots::EpochSlots, serve_repair::RepairType};

use solana_ledger::{bank_forks::BankForks, staking_utils};
use solana_sdk::{clock::Slot, pubkey::Pubkey};

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::RwLock,
};

#[derive(Default)]
pub struct ClusterSlots {
    cluster_slots: HashMap<Slot, HashMap<Rc<Pubkey>, u64>>,
    keys: HashSet<Rc<Pubkey>>,
    since: Option<u64>,
    validator_stakes: HashMap<Rc<Pubkey>, u64>,
    epoch: Option<u64>,
    self_id: Pubkey,
}

impl ClusterSlots {
    pub fn lookup(&self, slot: Slot) -> Option<&HashMap<Rc<Pubkey>, u64>> {
        self.cluster_slots.get(&slot)
    }
    pub fn update(
        &mut self,
        root: Slot,
        cluster_info: &RwLock<ClusterInfo>,
        bank_forks: &RwLock<BankForks>,
    ) {
        self.update_peers(cluster_info, bank_forks);
        let epoch_slots = cluster_info
            .read()
            .unwrap()
            .get_epoch_slots_since(self.since);
        self.update_internal(root, epoch_slots);
    }
    fn update_internal(&mut self, root: Slot, epoch_slots: (Vec<EpochSlots>, Option<u64>)) {
        let (epoch_slots_list, since) = epoch_slots;
        for epoch_slots in epoch_slots_list {
            let slots = epoch_slots.to_slots(root);
            for slot in &slots {
                if *slot <= root {
                    continue;
                }
                let pubkey = Rc::new(epoch_slots.from);
                if self.keys.get(&pubkey).is_none() {
                    self.keys.insert(pubkey.clone());
                }
                let from = self.keys.get(&pubkey).unwrap();
                let balance = self.validator_stakes.get(from).cloned().unwrap_or(0);
                if self.self_id != **from {
                    debug!(
                        "CLUSTER_SLLOTS: {}: insert {} {} {}",
                        self.self_id, from, *slot, balance
                    );
                }
                self.cluster_slots
                    .entry(*slot)
                    .or_insert_with(HashMap::default)
                    .insert(from.clone(), balance);
            }
        }
        self.cluster_slots.retain(|x, _| *x > root);
        self.keys.retain(|x| Rc::strong_count(x) > 1);
        self.since = since;
    }
    pub fn stats(&self) -> (usize, usize, f64) {
        let my_slots = self.collect(&self.self_id);
        let others: HashMap<_, _> = self
            .cluster_slots
            .iter()
            .filter(|(x, _)| !my_slots.contains(x))
            .flat_map(|(_, x)| x.iter())
            .collect();
        let other_slots: Vec<Slot> = self
            .cluster_slots
            .iter()
            .filter(|(x, _)| !my_slots.contains(x))
            .map(|(x, _)| *x)
            .collect();

        let weight: u64 = others.values().map(|x| **x).sum();
        let keys: Vec<Rc<Pubkey>> = others.keys().copied().cloned().collect();
        let total: u64 = self.validator_stakes.values().copied().sum::<u64>() + 1u64;
        if !other_slots.is_empty() {
            debug!(
                "{}: CLUSTER_SLOTS STATS {} {:?} {:?}",
                self.self_id,
                weight as f64 / total as f64,
                keys,
                other_slots
            );
        }
        (
            my_slots.len(),
            self.cluster_slots.len(),
            weight as f64 / total as f64,
        )
    }
    pub fn collect(&self, id: &Pubkey) -> HashSet<Slot> {
        self.cluster_slots
            .iter()
            .filter(|(_, keys)| keys.get(id).is_some())
            .map(|(slot, _)| slot)
            .cloned()
            .collect()
    }

    pub fn update_peers(
        &mut self,
        cluster_info: &RwLock<ClusterInfo>,
        bank_forks: &RwLock<BankForks>,
    ) {
        let root = bank_forks.read().unwrap().root();
        let (epoch, _) = bank_forks
            .read()
            .unwrap()
            .working_bank()
            .get_epoch_and_slot_index(root);
        if Some(epoch) != self.epoch {
            let stakes = staking_utils::staked_nodes_at_epoch(
                &bank_forks.read().unwrap().working_bank(),
                epoch,
            );
            if stakes.is_none() {
                return;
            }
            let stakes = stakes.unwrap();
            self.validator_stakes = HashMap::new();
            for (from, bal) in stakes {
                let pubkey = Rc::new(from);
                if self.keys.get(&pubkey).is_none() {
                    self.keys.insert(pubkey.clone());
                }
                let from = self.keys.get(&pubkey).unwrap();
                self.validator_stakes.insert(from.clone(), bal);
            }
            self.self_id = cluster_info.read().unwrap().id();
            self.epoch = Some(epoch);
        }
    }
    pub fn peers(&self, slot: Slot) -> Vec<(Rc<Pubkey>, u64)> {
        let mut peers: HashMap<Rc<Pubkey>, u64> = self.validator_stakes.clone();
        if let Some(slot_peers) = self.lookup(slot) {
            slot_peers
                .iter()
                .for_each(|(x, y)| *peers.entry(x.clone()).or_insert(0) += *y);
        }
        peers.into_iter().filter(|x| *x.0 != self.self_id).collect()
    }

    pub fn generate_repairs_for_missing_slots(
        &self,
        self_id: &Pubkey,
        root: Slot,
    ) -> Vec<RepairType> {
        let my_slots = self.collect(self_id);
        self.cluster_slots
            .keys()
            .filter(|x| **x > root)
            .filter(|x| !my_slots.contains(*x))
            .map(|x| RepairType::HighestShred(*x, 0))
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
        cs.update_internal(0, (vec![], None));
        assert!(cs.cluster_slots.is_empty());
        assert!(cs.since.is_none());
    }

    #[test]
    fn test_update_empty() {
        let mut cs = ClusterSlots::default();
        let epoch_slot = EpochSlots::default();
        cs.update_internal(0, (vec![epoch_slot], Some(0)));
        assert_eq!(cs.since, Some(0));
        assert!(cs.lookup(0).is_none());
    }

    #[test]
    fn test_update_rooted() {
        //root is 0, so it should clear out the slot
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.fill(&[0], 0);
        cs.update_internal(0, (vec![epoch_slot], Some(0)));
        assert_eq!(cs.since, Some(0));
        assert!(cs.lookup(0).is_none());
    }

    #[test]
    fn test_update_new_slot() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.fill(&[1], 0);
        cs.update_internal(0, (vec![epoch_slot], Some(0)));
        assert_eq!(cs.since, Some(0));
        assert!(cs.lookup(0).is_none());
        assert!(cs.lookup(1).is_some());
        assert_eq!(cs.lookup(1).unwrap().get(&Pubkey::default()), Some(&0));
    }

    #[test]
    fn test_update_new_staked_slot() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.fill(&[1], 0);
        let map = vec![(Rc::new(Pubkey::default()), 1)].into_iter().collect();
        cs.validator_stakes = map;
        cs.update_internal(0, (vec![epoch_slot], None));
        assert!(cs.lookup(1).is_some());
        assert_eq!(cs.lookup(1).unwrap().get(&Pubkey::default()), Some(&1));
    }

    #[test]
    fn test_generate_repairs() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.fill(&[1], 0);
        cs.update_internal(0, (vec![epoch_slot], None));
        let self_id = Pubkey::new_rand();
        assert_eq!(
            cs.generate_repairs_for_missing_slots(&self_id, 0),
            vec![RepairType::HighestShred(1, 0)]
        )
    }

    #[test]
    fn test_collect_my_slots() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.fill(&[1], 0);
        let self_id = epoch_slot.from;
        cs.update_internal(0, (vec![epoch_slot], None));
        let slots: Vec<Slot> = cs.collect(&self_id).into_iter().collect();
        assert_eq!(slots, vec![1]);
    }

    #[test]
    fn test_generate_repairs_existing() {
        let mut cs = ClusterSlots::default();
        let mut epoch_slot = EpochSlots::default();
        epoch_slot.fill(&[1], 0);
        let self_id = epoch_slot.from;
        cs.update_internal(0, (vec![epoch_slot], None));
        assert!(cs
            .generate_repairs_for_missing_slots(&self_id, 0)
            .is_empty());
    }
}
