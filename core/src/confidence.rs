use std::collections::{HashMap, HashSet};

#[derive(Debug, Default, PartialEq)]
pub struct Confidence {
    fork_stakes: u64,
    total_stake: u64,
    lockouts: u64,
    stake_weighted_lockouts: u128,
}

impl Confidence {
    pub fn new(fork_stakes: u64, total_stake: u64, lockouts: u64) -> Self {
        Self {
            fork_stakes,
            total_stake,
            lockouts,
            stake_weighted_lockouts: 0,
        }
    }
    pub fn new_with_stake_weighted(
        fork_stakes: u64,
        total_stake: u64,
        lockouts: u64,
        stake_weighted_lockouts: u128,
    ) -> Self {
        Self {
            fork_stakes,
            total_stake,
            lockouts,
            stake_weighted_lockouts,
        }
    }
}

#[derive(Default, PartialEq)]
pub struct ForkConfidenceCache {
    confidence: HashMap<u64, Confidence>,
}

impl ForkConfidenceCache {
    pub fn cache_fork_confidence(
        &mut self,
        fork: u64,
        fork_stakes: u64,
        total_stake: u64,
        lockouts: u64,
    ) {
        self.confidence
            .entry(fork)
            .and_modify(|entry| {
                entry.fork_stakes = fork_stakes;
                entry.total_stake = total_stake;
                entry.lockouts = lockouts;
            })
            .or_insert_with(|| Confidence::new(fork_stakes, total_stake, lockouts));
    }

    pub fn cache_stake_weighted_lockouts(&mut self, fork: u64, stake_weighted_lockouts: u128) {
        self.confidence
            .entry(fork)
            .and_modify(|entry| {
                entry.stake_weighted_lockouts = stake_weighted_lockouts;
            })
            .or_insert(Confidence {
                fork_stakes: 0,
                total_stake: 0,
                lockouts: 0,
                stake_weighted_lockouts,
            });
    }

    pub fn get_fork_confidence(&self, fork: u64) -> Option<&Confidence> {
        self.confidence.get(&fork)
    }

    pub fn prune_confidence_cache(&mut self, ancestors: &HashMap<u64, HashSet<u64>>, root: u64) {
        // For Every slot `s` in this cache must exist some bank `b` in BankForks with
        // `b.slot() == s`, and because `ancestors` has an entry for every bank in BankForks,
        // then there must be an entry in `ancestors` for every slot in `self.confidence`
        self.confidence
            .retain(|slot, _| slot == &root || ancestors[&slot].contains(&root));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fork_confidence_cache() {
        let mut cache = ForkConfidenceCache::default();
        let fork = 0;
        assert!(cache.confidence.get(&fork).is_none());
        cache.cache_fork_confidence(fork, 11, 12, 13);
        assert_eq!(
            cache.confidence.get(&fork).unwrap(),
            &Confidence {
                fork_stakes: 11,
                total_stake: 12,
                lockouts: 13,
                stake_weighted_lockouts: 0,
            }
        );
        // Ensure that {fork_stakes, total_stake, lockouts} and stake_weighted_lockouts
        // can be updated separately
        cache.cache_stake_weighted_lockouts(fork, 20);
        assert_eq!(
            cache.confidence.get(&fork).unwrap(),
            &Confidence {
                fork_stakes: 11,
                total_stake: 12,
                lockouts: 13,
                stake_weighted_lockouts: 20,
            }
        );
        cache.cache_fork_confidence(fork, 21, 22, 23);
        assert_eq!(
            cache.confidence.get(&fork).unwrap().stake_weighted_lockouts,
            20,
        );
    }
}
