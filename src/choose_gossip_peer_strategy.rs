use cluster_info::{ClusterInfoError, NodeInfo};
use rand::distributions::{Distribution, Weighted, WeightedChoice};
use rand::thread_rng;
use result::Result;
use solana_sdk::pubkey::Pubkey;
use std;
use std::collections::HashMap;

pub const DEFAULT_WEIGHT: u32 = 1;

pub trait ChooseGossipPeerStrategy {
    fn choose_peer<'a>(&self, options: Vec<&'a NodeInfo>) -> Result<&'a NodeInfo>;
}

pub struct ChooseRandomPeerStrategy<'a> {
    random: &'a Fn() -> u64,
}

// Given a source of randomness "random", this strategy will randomly pick a validator
// from the input options. This strategy works in isolation, but doesn't leverage any
// rumors from the rest of the gossip network to make more informed decisions about
// which validators have more/less updates
impl<'a, 'b> ChooseRandomPeerStrategy<'a> {
    pub fn new(random: &'a Fn() -> u64) -> Self {
        ChooseRandomPeerStrategy { random }
    }
}

impl<'a> ChooseGossipPeerStrategy for ChooseRandomPeerStrategy<'a> {
    fn choose_peer<'b>(&self, options: Vec<&'b NodeInfo>) -> Result<&'b NodeInfo> {
        if options.is_empty() {
            Err(ClusterInfoError::NoPeers)?;
        }

        let n = ((self.random)() as usize) % options.len();
        Ok(options[n])
    }
}

// This strategy uses rumors accumulated from the rest of the network to weight
// the importance of communicating with a particular validator based on cumulative network
// perceiption of the number of updates the validator has to offer. A validator is randomly
// picked based on a weighted sample from the pool of viable choices. The "weight", w, of a
// particular validator "v" is calculated as follows:
//
//  w = [Sum for all i in I_v: (rumor_v(i) - observed(v)) * stake(i)] /
//      [Sum for all i in I_v: Sum(stake(i))]
//
// where I_v is the set of all validators that returned a rumor about the update_index of
// validator "v", stake(i) is the size of the stake of validator "i", observed(v) is the
// observed update_index from the last direct communication validator "v", and
// rumor_v(i) is the rumored update_index of validator "v" propagated by fellow validator "i".

// This could be a problem if there are validators with large stakes lying about their
// observed updates. There could also be a problem in network partitions, or even just
// when certain validators are disproportionately active, where we hear more rumors about
// certain clusters of nodes that then propagate more rumros about each other. Hopefully
// this can be resolved with a good baseline DEFAULT_WEIGHT, or by implementing lockout
// periods for very active validators in the future.

pub struct ChooseWeightedPeerStrategy<'a> {
    // The map of last directly observed update_index for each active validator.
    // This is how we get observed(v) from the formula above.
    remote: &'a HashMap<Pubkey, u64>,
    // The map of rumored update_index for each active validator. Using the formula above,
    // to find rumor_v(i), we would first look up "v" in the outer map, then look up
    // "i" in the inner map, i.e. look up external_liveness[v][i]
    external_liveness: &'a HashMap<Pubkey, HashMap<Pubkey, u64>>,
    // A function returning the size of the stake for a particular validator, corresponds
    // to stake(i) in the formula above.
    get_stake: &'a Fn(Pubkey) -> f64,
}

impl<'a> ChooseWeightedPeerStrategy<'a> {
    pub fn new(
        remote: &'a HashMap<Pubkey, u64>,
        external_liveness: &'a HashMap<Pubkey, HashMap<Pubkey, u64>>,
        get_stake: &'a Fn(Pubkey) -> f64,
    ) -> Self {
        ChooseWeightedPeerStrategy {
            remote,
            external_liveness,
            get_stake,
        }
    }

    fn calculate_weighted_remote_index(&self, peer_id: Pubkey) -> u32 {
        let mut last_seen_index = 0;
        // If the peer is not in our remote table, then we leave last_seen_index as zero.
        // Only happens when a peer appears in our cluster_info.table but not in our cluster_info.remote,
        // which means a validator was directly injected into our cluster_info.table
        if let Some(index) = self.remote.get(&peer_id) {
            last_seen_index = *index;
        }

        let liveness_entry = self.external_liveness.get(&peer_id);
        if liveness_entry.is_none() {
            return DEFAULT_WEIGHT;
        }

        let votes = liveness_entry.unwrap();

        if votes.is_empty() {
            return DEFAULT_WEIGHT;
        }

        // Calculate the weighted average of the rumors
        let mut relevant_votes = vec![];

        let total_stake = votes.iter().fold(0.0, |total_stake, (&id, &vote)| {
            let stake = (self.get_stake)(id);
            // If the total stake is going to overflow u64, pick
            // the larger of either the current total_stake, or the
            // new stake, this way we are guaranteed to get at least u64/2
            // sample of stake in our weighted calculation
            if std::f64::MAX - total_stake < stake {
                if stake > total_stake {
                    relevant_votes = vec![(stake, vote)];
                    stake
                } else {
                    total_stake
                }
            } else {
                relevant_votes.push((stake, vote));
                total_stake + stake
            }
        });

        let weighted_vote = relevant_votes.iter().fold(0.0, |sum, &(stake, vote)| {
            if vote < last_seen_index {
                // This should never happen because we maintain the invariant that the indexes
                // in the external_liveness table are always greater than the corresponding
                // indexes in the remote table, if the index exists in the remote table at all.

                // Case 1: Attempt to insert bigger index into the "external_liveness" table
                // happens after an insertion into the "remote" table. In this case,
                // (see apply_updates()) function, we prevent the insertion if the entry
                // in the remote table >= the atempted insertion into the "external" liveness
                // table.

                // Case 2: Bigger index in the "external_liveness" table inserted before
                // a smaller insertion into the "remote" table. We clear the corresponding
                // "external_liveness" table entry on all insertions into the  "remote" table
                // See apply_updates() function.

                warn!("weighted peer index was smaller than local entry in remote table");
                return sum;
            }

            let vote_difference = (vote - last_seen_index) as f64;
            let new_weight = vote_difference * (stake / total_stake);

            if std::f64::MAX - sum < new_weight {
                return f64::max(new_weight, sum);
            }

            sum + new_weight
        });

        // Return u32 b/c the weighted sampling API from rand::distributions
        // only takes u32 for weights
        if weighted_vote >= f64::from(std::u32::MAX) {
            return std::u32::MAX;
        }

        // If the weighted rumors we've heard about aren't any greater than
        // what we've directly learned from the last time we communicated with the
        // peer (i.e. weighted_vote == 0), then return a weight of 1.
        // Otherwise, return the calculated weight.
        weighted_vote as u32 + DEFAULT_WEIGHT
    }
}

impl<'a> ChooseGossipPeerStrategy for ChooseWeightedPeerStrategy<'a> {
    fn choose_peer<'b>(&self, options: Vec<&'b NodeInfo>) -> Result<&'b NodeInfo> {
        if options.is_empty() {
            Err(ClusterInfoError::NoPeers)?;
        }

        let mut weighted_peers = vec![];
        for peer in options {
            let weight = self.calculate_weighted_remote_index(peer.id);
            weighted_peers.push(Weighted { weight, item: peer });
        }

        let mut rng = thread_rng();
        Ok(WeightedChoice::new(&mut weighted_peers).sample(&mut rng))
    }
}

#[cfg(test)]
mod tests {
    use choose_gossip_peer_strategy::{ChooseWeightedPeerStrategy, DEFAULT_WEIGHT};
    use logger;
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::pubkey::Pubkey;
    use std;
    use std::collections::HashMap;

    fn get_stake(_id: Pubkey) -> f64 {
        1.0
    }

    #[test]
    fn test_default() {
        logger::setup();

        // Initialize the filler keys
        let key1 = Keypair::new().pubkey();

        let remote: HashMap<Pubkey, u64> = HashMap::new();
        let external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>> = HashMap::new();

        let weighted_strategy =
            ChooseWeightedPeerStrategy::new(&remote, &external_liveness, &get_stake);

        // If external_liveness table doesn't contain this entry,
        // return the default weight
        let result = weighted_strategy.calculate_weighted_remote_index(key1);
        assert_eq!(result, DEFAULT_WEIGHT);
    }

    #[test]
    fn test_only_external_liveness() {
        logger::setup();

        // Initialize the filler keys
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();

        let remote: HashMap<Pubkey, u64> = HashMap::new();
        let mut external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>> = HashMap::new();

        // If only the liveness table contains the entry, should return the
        // weighted liveness entries
        let test_value: u32 = 5;
        let mut rumors: HashMap<Pubkey, u64> = HashMap::new();
        rumors.insert(key2, test_value as u64);
        external_liveness.insert(key1, rumors);

        let weighted_strategy =
            ChooseWeightedPeerStrategy::new(&remote, &external_liveness, &get_stake);

        let result = weighted_strategy.calculate_weighted_remote_index(key1);
        assert_eq!(result, test_value + DEFAULT_WEIGHT);
    }

    #[test]
    fn test_overflow_votes() {
        logger::setup();

        // Initialize the filler keys
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();

        let remote: HashMap<Pubkey, u64> = HashMap::new();
        let mut external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>> = HashMap::new();

        // If the vote index is greater than u32::MAX, default to u32::MAX
        let test_value = (std::u32::MAX as u64) + 10;
        let mut rumors: HashMap<Pubkey, u64> = HashMap::new();
        rumors.insert(key2, test_value);
        external_liveness.insert(key1, rumors);

        let weighted_strategy =
            ChooseWeightedPeerStrategy::new(&remote, &external_liveness, &get_stake);

        let result = weighted_strategy.calculate_weighted_remote_index(key1);
        assert_eq!(result, std::u32::MAX);
    }

    #[test]
    fn test_many_validators() {
        logger::setup();

        // Initialize the filler keys
        let key1 = Keypair::new().pubkey();

        let mut remote: HashMap<Pubkey, u64> = HashMap::new();
        let mut external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>> = HashMap::new();

        // Test many validators' rumors in external_liveness
        let num_peers = 10;
        let mut rumors: HashMap<Pubkey, u64> = HashMap::new();

        remote.insert(key1, 0);

        for i in 0..num_peers {
            let pubkey = Keypair::new().pubkey();
            rumors.insert(pubkey, i);
        }

        external_liveness.insert(key1, rumors);

        let weighted_strategy =
            ChooseWeightedPeerStrategy::new(&remote, &external_liveness, &get_stake);

        let result = weighted_strategy.calculate_weighted_remote_index(key1);
        assert_eq!(result, (num_peers / 2) as u32);
    }

    #[test]
    fn test_many_validators2() {
        logger::setup();

        // Initialize the filler keys
        let key1 = Keypair::new().pubkey();

        let mut remote: HashMap<Pubkey, u64> = HashMap::new();
        let mut external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>> = HashMap::new();

        // Test many validators' rumors in external_liveness
        let num_peers = 10;
        let old_index = 20;
        let mut rumors: HashMap<Pubkey, u64> = HashMap::new();

        remote.insert(key1, old_index);

        for _i in 0..num_peers {
            let pubkey = Keypair::new().pubkey();
            rumors.insert(pubkey, old_index);
        }

        external_liveness.insert(key1, rumors);

        let weighted_strategy =
            ChooseWeightedPeerStrategy::new(&remote, &external_liveness, &get_stake);

        let result = weighted_strategy.calculate_weighted_remote_index(key1);

        // If nobody has seen a newer update then revert to default
        assert_eq!(result, DEFAULT_WEIGHT);
    }
}
