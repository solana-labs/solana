use crate::result::{Error, Result};
use crate::service::Service;
use solana_runtime::bank::Bank;
use solana_vote_api::vote_state::VoteState;
use solana_vote_api::vote_state::MAX_LOCKOUT_HISTORY;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub struct BankConfidence {
    confidence: Vec<u64>,
}

impl BankConfidence {
    pub fn new() -> Self {
        Self {
            confidence: vec![0u64; MAX_LOCKOUT_HISTORY],
        }
    }

    pub fn increase_confirmation_count(&mut self, confirmation_count: usize, stake: u64) {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.confidence[confirmation_count - 1] += stake;
    }

    pub fn get_confirmation_stake(&mut self, confirmation_count: usize) -> u64 {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.confidence[confirmation_count - 1]
    }
}

#[derive(Default)]
pub struct ForkConfidenceCache {
    bank_confidence: HashMap<u64, BankConfidence>,
    total_stake: u64,
}

impl ForkConfidenceCache {
    pub fn new(bank_confidence: HashMap<u64, BankConfidence>, total_stake: u64) -> Self {
        Self {
            bank_confidence,
            total_stake,
        }
    }

    pub fn get_fork_confidence(&self, fork: u64) -> Option<&BankConfidence> {
        self.bank_confidence.get(&fork)
    }
}

pub struct ConfidenceAggregationData {
    bank: Arc<Bank>,
    total_staked: u64,
}

impl ConfidenceAggregationData {
    pub fn new(bank: Arc<Bank>, total_staked: u64) -> Self {
        Self { bank, total_staked }
    }
}

pub struct AggregateConfidenceService {
    t_confidence: JoinHandle<()>,
}

impl AggregateConfidenceService {
    pub fn new(
        exit: &Arc<AtomicBool>,
        fork_confidence_cache: Arc<RwLock<ForkConfidenceCache>>,
    ) -> (Sender<ConfidenceAggregationData>, Self) {
        let (sender, receiver): (
            Sender<ConfidenceAggregationData>,
            Receiver<ConfidenceAggregationData>,
        ) = channel();
        let exit_ = exit.clone();
        (
            sender,
            Self {
                t_confidence: Builder::new()
                    .name("solana-aggregate-stake-lockouts".to_string())
                    .spawn(move || loop {
                        if exit_.load(Ordering::Relaxed) {
                            break;
                        }

                        if let Err(e) = Self::run(&receiver, &fork_confidence_cache, &exit_) {
                            match e {
                                Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                                Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                                _ => info!(
                                    "Unexpected error from AggregateConfidenceService: {:?}",
                                    e
                                ),
                            }
                        }
                    })
                    .unwrap(),
            },
        )
    }

    fn run(
        receiver: &Receiver<ConfidenceAggregationData>,
        fork_confidence_cache: &RwLock<ForkConfidenceCache>,
        exit: &Arc<AtomicBool>,
    ) -> Result<()> {
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            let mut aggregation_data = receiver.recv_timeout(Duration::from_secs(1))?;

            while let Ok(new_data) = receiver.try_recv() {
                aggregation_data = new_data;
            }

            let ancestors = aggregation_data.bank.status_cache_ancestors();
            if ancestors.len() == 0 {
                continue;
            }

            let bank_confidence = Self::aggregate_confidence(&ancestors, &aggregation_data.bank);

            let mut new_fork_confidence =
                ForkConfidenceCache::new(bank_confidence, aggregation_data.total_staked);

            let mut w_fork_confidence_cache = fork_confidence_cache.write().unwrap();

            std::mem::swap(&mut *w_fork_confidence_cache, &mut new_fork_confidence);
        }
    }

    pub fn aggregate_confidence(ancestors: &[u64], bank: &Bank) -> HashMap<u64, BankConfidence> {
        assert!(ancestors.len() > 0);

        // Check ancestors is sorted
        for a in ancestors.windows(2) {
            assert!(a[0] < a[1]);
        }

        let mut confidence = HashMap::new();
        for (_, (lamports, account)) in bank.vote_accounts().into_iter() {
            if lamports == 0 {
                continue;
            }
            let vote_state = VoteState::from(&account);
            if vote_state.is_none() {
                continue;
            }

            let vote_state = vote_state.unwrap();
            Self::aggregate_confidence_for_vote_account(
                &mut confidence,
                &vote_state,
                ancestors,
                lamports,
            );
        }

        confidence
    }

    fn aggregate_confidence_for_vote_account(
        confidence: &mut HashMap<u64, BankConfidence>,
        vote_state: &VoteState,
        ancestors: &[u64],
        lamports: u64,
    ) {
        assert!(ancestors.len() > 0);
        let mut ancestors_index = 0;
        if let Some(root) = vote_state.root_slot {
            for (i, a) in ancestors.iter().enumerate() {
                if *a <= root {
                    confidence
                        .entry(*a)
                        .or_insert(BankConfidence::new())
                        .increase_confirmation_count(MAX_LOCKOUT_HISTORY, lamports);
                } else {
                    ancestors_index = i;
                    break;
                }
            }
        }

        for vote in &vote_state.votes {
            while ancestors[ancestors_index] <= vote.slot {
                confidence
                    .entry(ancestors[ancestors_index])
                    .or_insert(BankConfidence::new())
                    .increase_confirmation_count(vote.confirmation_count as usize, lamports);
                ancestors_index += 1;

                if ancestors_index == ancestors.len() - 1 {
                    return;
                }
            }
        }
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
    use crate::consensus::StakeLockout;

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
