use crate::consensus::StakeLockout;
use crate::service::Service;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};

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

pub struct ConfidenceAggregationData {
    lockouts: HashMap<u64, StakeLockout>,
    root: Option<u64>,
    ancestors: Arc<HashMap<u64, HashSet<u64>>>,
    total_staked: u64,
}

impl ConfidenceAggregationData {
    pub fn new(
        lockouts: HashMap<u64, StakeLockout>,
        root: Option<u64>,
        ancestors: Arc<HashMap<u64, HashSet<u64>>>,
        total_staked: u64,
    ) -> Self {
        Self {
            lockouts,
            root,
            ancestors,
            total_staked,
        }
    }
}

pub struct AggregateConfidenceService {
    t_confidence: JoinHandle<()>,
}

impl AggregateConfidenceService {
    pub fn aggregate_confidence(
        root: Option<u64>,
        ancestors: &HashMap<u64, HashSet<u64>>,
        stake_lockouts: &HashMap<u64, StakeLockout>,
    ) -> HashMap<u64, u128> {
        let mut stake_weighted_lockouts: HashMap<u64, u128> = HashMap::new();
        for (fork, lockout) in stake_lockouts.iter() {
            if root.is_none() || *fork >= root.unwrap() {
                let mut slot_with_ancestors = vec![*fork];
                slot_with_ancestors.extend(ancestors.get(&fork).unwrap_or(&HashSet::new()));
                for slot in slot_with_ancestors {
                    if root.is_none() || slot >= root.unwrap() {
                        let entry = stake_weighted_lockouts.entry(slot).or_default();
                        *entry += u128::from(lockout.lockout()) * u128::from(lockout.stake());
                    }
                }
            }
        }
        stake_weighted_lockouts
    }

    pub fn new(
        exit: &Arc<AtomicBool>,
        fork_confidence_cache: Arc<RwLock<ForkConfidenceCache>>,
    ) -> (Sender<ConfidenceAggregationData>, Self) {
        let (lockouts_sender, lockouts_receiver): (
            Sender<ConfidenceAggregationData>,
            Receiver<ConfidenceAggregationData>,
        ) = channel();
        let exit_ = exit.clone();
        (
            lockouts_sender,
            Self {
                t_confidence: Builder::new()
                    .name("solana-aggregate-stake-lockouts".to_string())
                    .spawn(move || loop {
                        if exit_.load(Ordering::Relaxed) {
                            break;
                        }
                        if let Ok(aggregation_data) = lockouts_receiver.try_recv() {
                            let stake_weighted_lockouts = Self::aggregate_confidence(
                                aggregation_data.root,
                                &aggregation_data.ancestors,
                                &aggregation_data.lockouts,
                            );

                            let mut w_fork_confidence_cache =
                                fork_confidence_cache.write().unwrap();

                            // Cache the confidence values
                            for (fork, stake_lockout) in aggregation_data.lockouts.iter() {
                                if aggregation_data.root.is_none()
                                    || *fork >= aggregation_data.root.unwrap()
                                {
                                    w_fork_confidence_cache.cache_fork_confidence(
                                        *fork,
                                        stake_lockout.stake(),
                                        aggregation_data.total_staked,
                                        stake_lockout.lockout(),
                                    );
                                }
                            }

                            // Cache the stake weighted lockouts
                            for (fork, stake_weighted_lockout) in stake_weighted_lockouts.iter() {
                                if aggregation_data.root.is_none()
                                    || *fork >= aggregation_data.root.unwrap()
                                {
                                    w_fork_confidence_cache.cache_stake_weighted_lockouts(
                                        *fork,
                                        *stake_weighted_lockout,
                                    )
                                }
                            }

                            if let Some(root) = aggregation_data.root {
                                w_fork_confidence_cache
                                    .prune_confidence_cache(&aggregation_data.ancestors, root);
                            }

                            drop(w_fork_confidence_cache);
                        }
                    })
                    .unwrap(),
            },
        )
    }
}

impl Service for AggregateConfidenceService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_confidence.join()
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

    #[test]
    fn test_aggregate_confidence() {
        let stakes = vec![
            (0, StakeLockout::new(1, 32)),
            (1, StakeLockout::new(1, 24)),
            (2, StakeLockout::new(1, 16)),
            (3, StakeLockout::new(1, 8)),
        ]
        .into_iter()
        .collect();
        let ancestors = vec![
            (0, HashSet::new()),
            (1, vec![0].into_iter().collect()),
            (2, vec![0, 1].into_iter().collect()),
            (3, vec![0, 1, 2].into_iter().collect()),
        ]
        .into_iter()
        .collect();
        let stake_weighted_lockouts =
            AggregateConfidenceService::aggregate_confidence(Some(1), &ancestors, &stakes);
        assert!(stake_weighted_lockouts.get(&0).is_none());
        assert_eq!(*stake_weighted_lockouts.get(&1).unwrap(), 8 + 16 + 24);
        assert_eq!(*stake_weighted_lockouts.get(&2).unwrap(), 8 + 16);
        assert_eq!(*stake_weighted_lockouts.get(&3).unwrap(), 8);
    }
}
