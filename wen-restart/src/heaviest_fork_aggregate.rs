use {
    crate::epoch_stakes_map::EpochStakesMap,
    log::*,
    solana_gossip::crds_value::Percent,
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    std::collections::{HashMap, HashSet},
};

pub struct HeaviestForkAggregate {
    my_slot: Slot,
    my_hash: Hash,
    active_peers: HashSet<Pubkey>,
    aggregate: HashMap<(Slot, Hash), HashSet<Pubkey>>,
}

impl HeaviestForkAggregate {
    pub(crate) fn new(my_pubkey: Pubkey, my_slot: Slot, my_hash: Hash) -> Self {
        let mut active_peers = HashSet::new();
        active_peers.insert(my_pubkey);
        let mut aggregate = HashMap::new();
        aggregate.insert((my_slot, my_hash), active_peers.clone());
        Self {
            my_slot,
            my_hash,
            active_peers,
            aggregate,
        }
    }

    pub(crate) fn aggregate(
        &mut self,
        heaviest_fork_list: Vec<(Slot, Hash, Percent)>,
        epoch_stakes_map: &mut EpochStakesMap,
    ) -> (f64, usize) {
        let node_stakes = epoch_stakes_map.epoch_stakes(None);
        let total_epoch_stake = node_stakes.total_stake();
        heaviest_fork_list
            .into_iter()
            .for_each(|(slot, hash, percent)| {
                let pubkey = percent.from;
                self.aggregate.entry((slot, hash)).or_insert(HashSet::new()).insert(pubkey);
                self.active_peers.insert(pubkey);
            });
        let mut stake_on_my_vote = 0;
        for ((slot, hash), peers) in self.aggregate.iter() {
            let total_stake = peers.iter().map(|pubkey| {
                node_stakes
                    .node_id_to_vote_accounts()
                    .get(pubkey)
                    .map(|node| node.total_stake)
                    .unwrap_or(0)
            })
            .reduce(|a, b| a + b)
            .unwrap();
            if slot == &self.my_slot && hash == &self.my_hash {
                stake_on_my_vote = total_stake;
            }
            info!("HeaviestFork: ({}, {}) : {} {:?}", slot, hash, total_stake, peers);
        }
        return (stake_on_my_vote as f64 / total_epoch_stake as f64, self.active_peers.len());
    }
}
