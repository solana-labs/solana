use {
    crate::epoch_stakes_map::EpochStakesMap,
    solana_gossip::{crds_value::EpochSlotsIndex, epoch_slots::EpochSlots},
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    std::collections::{BTreeMap, HashSet},
};

pub type SlotsToRepairList = Vec<(Slot, f64)>;

// We don't need locks here because update() and send_repairs() are called in a
// single-thread loop.
pub struct LastVotedForkSlotsAggregate {
    root_slot: Slot,
    slots_aggregate: BTreeMap<Slot, HashSet<Pubkey>>,
    active_peers: HashSet<Pubkey>,
}

impl LastVotedForkSlotsAggregate {
    pub(crate) fn new(root_slot: Slot, my_slots: &[Slot], my_pubkey: &Pubkey) -> Self {
        let mut slots_aggregate = BTreeMap::new();
        let mut active_peers = HashSet::new();
        for slot in my_slots {
            let mut new_set = HashSet::new();
            new_set.insert(my_pubkey.clone());
            slots_aggregate.insert(*slot, new_set);
        }
        active_peers.insert(*my_pubkey);
        Self {
            root_slot,
            slots_aggregate,
            active_peers,
        }
    }

    pub(crate) fn aggregate(
        &mut self,
        last_voted_fork_slots: Vec<(EpochSlotsIndex, EpochSlots, Slot, Hash)>,
        epoch_stakes_map: &mut EpochStakesMap,
    ) -> (Option<SlotsToRepairList>, f64, usize) {
        let node_stakes = epoch_stakes_map.epoch_stakes(None);
        let mut changed_slots = HashSet::new();
        last_voted_fork_slots
            .into_iter()
            .for_each(|(_, epoch_slots, _, _)| {
                let from = epoch_slots.from;
                self.active_peers.insert(from);
                epoch_slots
                    .to_slots(self.root_slot)
                    .into_iter()
                    .for_each(|slot| {
                        changed_slots.insert(slot);
                        match self.slots_aggregate.get_mut(&slot) {
                            Some(value) => value.insert(from),
                            None => {
                                let mut new_set = HashSet::new();
                                new_set.insert(from);
                                self.slots_aggregate.insert(slot, new_set).is_some()
                            }
                        };
                    });
            });
        let total_active_stake = self
            .active_peers
            .iter()
            .map(
                |pubkey| match node_stakes.node_id_to_vote_accounts().get(pubkey) {
                    Some(node_vote_accounts) => node_vote_accounts.total_stake,
                    None => 0,
                },
            )
            .reduce(|a, b| a + b)
            .unwrap();
        let not_active_percenage =
            1.0 - (total_active_stake as f64 / node_stakes.total_stake() as f64);
        let threshold_for_repair = 0.62 - not_active_percenage;
        if threshold_for_repair > 0.1 {
            let slots_to_repair = self
                .slots_aggregate
                .iter()
                .filter_map(|(slot, pubkeys)| {
                    let node_stakes_at_slot = epoch_stakes_map.epoch_stakes(Some(slot.clone()));
                    let total_slot_stake = pubkeys
                        .iter()
                        .map(|pubkey| {
                            match node_stakes_at_slot.node_id_to_vote_accounts().get(pubkey) {
                                Some(node_id_to_vote_accounts) => {
                                    node_id_to_vote_accounts.total_stake
                                }
                                None => 0,
                            }
                        })
                        .reduce(|a, b| a + b)
                        .unwrap();
                    let my_percent =
                        total_slot_stake as f64 / node_stakes_at_slot.total_stake() as f64;
                    if my_percent > threshold_for_repair {
                        Some((slot.clone(), my_percent))
                    } else {
                        None
                    }
                })
                .collect::<Vec<(Slot, f64)>>();
            (Some(slots_to_repair), not_active_percenage, self.active_peers.len())
        } else {
            (None, not_active_percenage, self.active_peers.len())
        }
    }
}
