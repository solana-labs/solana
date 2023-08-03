use {
    crate::epoch_stakes_map::EpochStakesMap,
    solana_gossip::crds_value::Percent,
    solana_runtime::bank::Bank,
    solana_sdk::{
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{
        collections::HashSet,
        sync::{Arc, RwLock},
    },
};

pub struct HeaviestForkAggregate {
    my_slot: Slot,
    my_hash: Hash,
    active_peers: RwLock<HashSet<Pubkey>>,
}

impl HeaviestForkAggregate {
    pub(crate) fn new(
        my_pubkey: Pubkey,
        my_slot: Slot,
        my_hash: Hash,
    ) -> Self {
        let mut peers = HashSet::new();
        peers.insert(my_pubkey);
        Self {
            my_slot,
            my_hash,
            active_peers: RwLock::new(peers),
        }
    }

    pub(crate) fn aggregate(&self, heaviest_fork_list: Vec<(Slot, Hash, Percent)>, epoch_stakes_map: &mut EpochStakesMap) -> f64 {
        let node_stakes = epoch_stakes_map.epoch_stakes(None);
        let total_epoch_stake = node_stakes.total_stake();
        let mut active_peers = self.active_peers.write().unwrap();
        heaviest_fork_list
            .into_iter()
            .for_each(|(slot, hash, percent)| {
                if slot == self.my_slot && hash == self.my_hash {
                    active_peers.insert(percent.from);
                }
            });
        let total_active_stake = active_peers
            .iter()
            .map(|pubkey| {
                node_stakes
                    .node_id_to_vote_accounts()
                    .get(pubkey)
                    .map(|node| node.total_stake)
                    .unwrap_or(0)
            })
            .reduce(|a, b| a + b)
            .unwrap();
        return total_active_stake as f64 / total_epoch_stake as f64;
    }
}
