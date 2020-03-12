use crate::{
    cluster_info::ClusterInfo, contact_info::ContactInfo, epoch_slots::EpochSlots,
    serve_repair::RepairType,
};

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

    fn update_peers(&mut self, cluster_info: &RwLock<ClusterInfo>, bank_forks: &RwLock<BankForks>) {
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

    pub fn compute_weights(&self, slot: Slot, repair_peers: &[ContactInfo]) -> Vec<(u64, usize)> {
        let slot_peers = self.lookup(slot);
        repair_peers
            .iter()
            .enumerate()
            .map(|(i, x)| {
                (
                    1 + slot_peers.and_then(|v| v.get(&x.id)).cloned().unwrap_or(0)
                        + self.validator_stakes.get(&x.id).cloned().unwrap_or(0),
                    i,
                )
            })
            .collect()
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
    fn test_compute_weights() {
        let cs = ClusterSlots::default();
        let ci = ContactInfo::default();
        assert_eq!(cs.compute_weights(0, &[ci]), vec![(1, 0)]);
    }

    #[test]
    fn test_best_peer_2() {
        let mut cs = ClusterSlots::default();
        let mut c1 = ContactInfo::default();
        let mut c2 = ContactInfo::default();
        let mut map = HashMap::new();
        let k1 = Pubkey::new_rand();
        let k2 = Pubkey::new_rand();
        map.insert(Rc::new(k1.clone()), std::u64::MAX / 2);
        map.insert(Rc::new(k2.clone()), 0);
        cs.cluster_slots.insert(0, map);
        c1.id = k1;
        c2.id = k2;
        assert_eq!(
            cs.compute_weights(0, &[c1, c2]),
            vec![(std::u64::MAX / 2 + 1, 0), (1, 1)]
        );
    }

    #[test]
    fn test_best_peer_3() {
        let mut cs = ClusterSlots::default();
        let mut c1 = ContactInfo::default();
        let mut c2 = ContactInfo::default();
        let mut map = HashMap::new();
        let k1 = Pubkey::new_rand();
        let k2 = Pubkey::new_rand();
        map.insert(Rc::new(k2.clone()), 0);
        cs.cluster_slots.insert(0, map);
        //make sure default weights are used as well
        cs.validator_stakes
            .insert(Rc::new(k1.clone()), std::u64::MAX / 2);
        c1.id = k1;
        c2.id = k2;
        assert_eq!(
            cs.compute_weights(0, &[c1, c2]),
            vec![(std::u64::MAX / 2 + 1, 0), (1, 1)]
        );
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
