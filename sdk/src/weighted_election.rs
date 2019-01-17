//! An election object where the weight of each vote is determined upfront.

use crate::pubkey::Pubkey;
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
pub struct Voter {
    voted: bool,
    weight: u64,
}

impl Voter {
    fn new(weight: u64) -> Self {
        let voted = false;
        Self { voted, weight }
    }
}

#[derive(Debug, PartialEq)]
pub enum WeightedElectionError {
    VoterIdNotFound, // The voter was not registered at the time the election began.
}

#[derive(Serialize, Deserialize)]
pub struct WeightedElection {
    voters: HashMap<Pubkey, Voter>,
}

impl WeightedElection {
    pub fn new(weights: HashMap<Pubkey, u64>) -> Self {
        let voters: HashMap<_, _> = weights
            .into_iter()
            .map(|(k, v)| (k, Voter::new(v)))
            .collect();
        Self { voters }
    }

    /// Return the combined weight of all potential voters.
    pub fn total_weight(&self) -> u64 {
        self.voters.iter().map(|(_, voter)| voter.weight).sum()
    }

    /// Return the combined weight of all voters that voted.
    pub fn voted_weight(&self) -> u64 {
        self.voters
            .iter()
            .map(|(_, voter)| if voter.voted { voter.weight } else { 0 })
            .sum()
    }

    /// Process a vote from `voter_id`.
    pub fn vote(&mut self, voter_id: &Pubkey) -> Result<(), WeightedElectionError> {
        match self.voters.get_mut(voter_id) {
            None => Err(WeightedElectionError::VoterIdNotFound),
            Some(ref mut voter) => {
                voter.voted = true;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_total_weight() {
        let weights = HashMap::new();
        assert_eq!(WeightedElection::new(weights).total_weight(), 0);

        let mut weights = HashMap::new();
        weights.insert(Keypair::new().pubkey(), 1);
        assert_eq!(WeightedElection::new(weights).total_weight(), 1);

        let mut weights = HashMap::new();
        weights.insert(Keypair::new().pubkey(), 1);
        weights.insert(Keypair::new().pubkey(), 2);
        assert_eq!(WeightedElection::new(weights).total_weight(), 3);
    }

    #[test]
    fn test_voted_weight() {
        let alice = Keypair::new().pubkey();
        let mut weights = HashMap::new();
        weights.insert(alice, 1);

        let mut election = WeightedElection::new(weights);
        assert_eq!(election.voted_weight(), 0);

        election.vote(&alice).unwrap();
        assert_eq!(election.voted_weight(), 1);

        // Ensure duplicate votes don't change voted weight.
        election.vote(&alice).unwrap();
        assert_eq!(election.voted_weight(), 1);

        // Ensure unrecognized pubkeys return an error.
        assert_eq!(
            election.vote(&Keypair::new().pubkey()),
            Err(WeightedElectionError::VoterIdNotFound)
        );
    }
}
