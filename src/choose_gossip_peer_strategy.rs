use crdt::ReplicatedData;
use rand::distributions::{IndependentSample, Weighted, WeightedChoice};
use rand::thread_rng;
use result::{Error, Result};
use signature::PublicKey;
use std;
use std::collections::HashMap;

pub const DEFAULT_WEIGHT: u32 = 1;

pub trait ChooseGossipPeerStrategy {
    fn choose_peer(&self, options: Vec<&ReplicatedData>) -> Result<ReplicatedData>;
}

pub struct ChooseRandomPeerStrategy<'a> {
    random: &'a Fn() -> u64,
}

impl<'a> ChooseRandomPeerStrategy<'a> {
    pub fn new(random: &'a Fn() -> u64) -> Self {
        ChooseRandomPeerStrategy { random }
    }
}

impl<'a> ChooseGossipPeerStrategy for ChooseRandomPeerStrategy<'a> {
    fn choose_peer(&self, options: Vec<&ReplicatedData>) -> Result<ReplicatedData> {
        if options.len() < 1 {
            return Err(Error::CrdtTooSmall);
        }

        let n = ((self.random)() as usize) % options.len();
        Ok(options[n].clone())
    }
}

pub struct ChooseWeightedPeerStrategy<'a> {
    remote: &'a HashMap<PublicKey, u64>,
    external_liveness: &'a HashMap<PublicKey, HashMap<PublicKey, u64>>,
    get_stake: &'a Fn(PublicKey) -> f64,
}

impl<'a> ChooseWeightedPeerStrategy<'a> {
    pub fn new(
        remote: &'a HashMap<PublicKey, u64>,
        external_liveness: &'a HashMap<PublicKey, HashMap<PublicKey, u64>>,
        get_stake: &'a Fn(PublicKey) -> f64,
    ) -> Self {
        ChooseWeightedPeerStrategy {
            remote,
            external_liveness,
            get_stake,
        }
    }

    fn calculate_weighted_remote_index(&self, peer_id: PublicKey) -> u32 {
        let mut last_seen_index = 0;
        // If the peer is not in our remote table, then we leave last_seen_index as zero.
        // Only happens when a peer appears in our crdt.table but not in our crdt.remote,
        // which means a validator was directly injected into our crdt.table
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
                // This should never happen b/c we maintain the invariant that the indexes
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
        if weighted_vote >= std::u32::MAX as f64 {
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
    fn choose_peer(&self, options: Vec<&ReplicatedData>) -> Result<ReplicatedData> {
        if options.len() < 1 {
            return Err(Error::CrdtTooSmall);
        }

        let mut weighted_peers = vec![];
        for peer in options {
            let weight = self.calculate_weighted_remote_index(peer.id);
            weighted_peers.push(Weighted {
                weight: weight,
                item: peer,
            });
        }

        let mut rng = thread_rng();
        Ok(WeightedChoice::new(&mut weighted_peers)
            .ind_sample(&mut rng)
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use choose_gossip_peer_strategy::{ChooseWeightedPeerStrategy, DEFAULT_WEIGHT};
    use logger;
    use signature::{KeyPair, KeyPairUtil, PublicKey};
    use std;
    use std::collections::HashMap;

    fn get_stake(id: PublicKey) -> f64 {
        return 1.0;
    }

    #[test]
    fn test_default() {
        logger::setup();

        // Initialize the filler keys
        let key1 = KeyPair::new().pubkey();

        let remote: HashMap<PublicKey, u64> = HashMap::new();
        let external_liveness: HashMap<PublicKey, HashMap<PublicKey, u64>> = HashMap::new();

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
        let key1 = KeyPair::new().pubkey();
        let key2 = KeyPair::new().pubkey();

        let remote: HashMap<PublicKey, u64> = HashMap::new();
        let mut external_liveness: HashMap<PublicKey, HashMap<PublicKey, u64>> = HashMap::new();

        // If only the liveness table contains the entry, should return the
        // weighted liveness entries
        let test_value: u32 = 5;
        let mut rumors: HashMap<PublicKey, u64> = HashMap::new();
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
        let key1 = KeyPair::new().pubkey();
        let key2 = KeyPair::new().pubkey();

        let remote: HashMap<PublicKey, u64> = HashMap::new();
        let mut external_liveness: HashMap<PublicKey, HashMap<PublicKey, u64>> = HashMap::new();

        // If the vote index is greater than u32::MAX, default to u32::MAX
        let test_value = (std::u32::MAX as u64) + 10;
        let mut rumors: HashMap<PublicKey, u64> = HashMap::new();
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
        let key1 = KeyPair::new().pubkey();

        let mut remote: HashMap<PublicKey, u64> = HashMap::new();
        let mut external_liveness: HashMap<PublicKey, HashMap<PublicKey, u64>> = HashMap::new();

        // Test many validators' rumors in external_liveness
        let num_peers = 10;
        let mut rumors: HashMap<PublicKey, u64> = HashMap::new();

        remote.insert(key1, 0);

        for i in 0..num_peers {
            let pk = KeyPair::new().pubkey();
            rumors.insert(pk, i);
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
        let key1 = KeyPair::new().pubkey();

        let mut remote: HashMap<PublicKey, u64> = HashMap::new();
        let mut external_liveness: HashMap<PublicKey, HashMap<PublicKey, u64>> = HashMap::new();

        // Test many validators' rumors in external_liveness
        let num_peers = 10;
        let old_index = 20;
        let mut rumors: HashMap<PublicKey, u64> = HashMap::new();

        remote.insert(key1, old_index);

        for i in 0..num_peers {
            let pk = KeyPair::new().pubkey();
            rumors.insert(pk, old_index);
        }

        external_liveness.insert(key1, rumors);

        let weighted_strategy =
            ChooseWeightedPeerStrategy::new(&remote, &external_liveness, &get_stake);

        let result = weighted_strategy.calculate_weighted_remote_index(key1);

        // If nobody has seen a newer update then rever to default
        assert_eq!(result, DEFAULT_WEIGHT);
    }
}
