use {
    crate::epoch_stakes_map::EpochStakesMap,
    solana_gossip::crds_value::Percent,
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    std::collections::HashSet,
};

pub struct HeaviestForkAggregate {
    my_slot: Slot,
    my_hash: Hash,
    active_peers: HashSet<Pubkey>,
}

impl HeaviestForkAggregate {
    pub(crate) fn new(my_pubkey: Pubkey, my_slot: Slot, my_hash: Hash) -> Self {
        let mut peers = HashSet::new();
        peers.insert(my_pubkey);
        Self {
            my_slot,
            my_hash,
            active_peers: peers,
        }
    }

    pub(crate) fn aggregate(
        &mut self,
        heaviest_fork_list: Vec<(Slot, Hash, Percent)>,
        epoch_stakes_map: &mut EpochStakesMap,
    ) -> f64 {
        let node_stakes = epoch_stakes_map.epoch_stakes(None);
        let total_epoch_stake = node_stakes.total_stake();
        heaviest_fork_list
            .into_iter()
            .for_each(|(slot, hash, percent)| {
                if slot == self.my_slot && hash == self.my_hash {
                    self.active_peers.insert(percent.from);
                }
            });
        let total_active_stake = self
            .active_peers
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
