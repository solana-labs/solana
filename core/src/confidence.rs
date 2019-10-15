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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BankConfidence {
    confidence: [u64; MAX_LOCKOUT_HISTORY],
}

impl BankConfidence {
    pub fn increase_confirmation_stake(&mut self, confirmation_count: usize, stake: u64) {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.confidence[confirmation_count - 1] += stake;
    }

    pub fn get_confirmation_stake(&mut self, confirmation_count: usize) -> u64 {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.confidence[confirmation_count - 1]
    }
    #[cfg(test)]
    pub(crate) fn new(confidence: [u64; MAX_LOCKOUT_HISTORY]) -> Self {
        Self { confidence }
    }
}

#[derive(Debug, Default)]
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

    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    pub fn get_fork_with_depth_confidence(
        &self,
        minimum_depth: usize,
        minimum_stake_percentage: f64,
    ) -> Option<u64> {
        self.bank_confidence
            .iter()
            .filter(|&(_, bank_confidence)| {
                let fork_stake_minimum_depth: u64 = bank_confidence.confidence[minimum_depth..]
                    .iter()
                    .cloned()
                    .sum();
                fork_stake_minimum_depth as f64 / self.total_stake as f64
                    >= minimum_stake_percentage
            })
            .map(|(slot, _)| *slot)
            .max()
    }

    pub fn get_rooted_fork_with_confidence(&self, minimum_stake_percentage: f64) -> Option<u64> {
        self.get_fork_with_depth_confidence(MAX_LOCKOUT_HISTORY - 1, minimum_stake_percentage)
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
            if ancestors.is_empty() {
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
        assert!(!ancestors.is_empty());

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
        assert!(!ancestors.is_empty());
        let mut ancestors_index = 0;
        if let Some(root) = vote_state.root_slot {
            for (i, a) in ancestors.iter().enumerate() {
                if *a <= root {
                    confidence
                        .entry(*a)
                        .or_insert_with(BankConfidence::default)
                        .increase_confirmation_stake(MAX_LOCKOUT_HISTORY, lamports);
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
                    .or_insert_with(BankConfidence::default)
                    .increase_confirmation_stake(vote.confirmation_count as usize, lamports);
                ancestors_index += 1;

                if ancestors_index == ancestors.len() {
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
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use solana_sdk::pubkey::Pubkey;
    use solana_stake_api::stake_state;
    use solana_vote_api::vote_state;

    #[test]
    fn test_bank_confidence() {
        let mut cache = BankConfidence::default();
        assert_eq!(cache.get_confirmation_stake(1), 0);
        cache.increase_confirmation_stake(1, 10);
        assert_eq!(cache.get_confirmation_stake(1), 10);
        cache.increase_confirmation_stake(1, 20);
        assert_eq!(cache.get_confirmation_stake(1), 30);
    }

    #[test]
    fn test_get_fork_with_depth_confidence() {
        // Build ForkConfidenceCache with votes at depths 0 and 1 for 2 slots
        let mut cache0 = BankConfidence::default();
        cache0.increase_confirmation_stake(1, 15);
        cache0.increase_confirmation_stake(2, 25);

        let mut cache1 = BankConfidence::default();
        cache1.increase_confirmation_stake(1, 10);
        cache1.increase_confirmation_stake(2, 20);

        let mut bank_confidence = HashMap::new();
        bank_confidence.entry(0).or_insert(cache0.clone());
        bank_confidence.entry(1).or_insert(cache1.clone());
        let fork_confidence_cache = ForkConfidenceCache::new(bank_confidence, 50);

        // Neither slot has rooted votes
        assert_eq!(
            fork_confidence_cache.get_rooted_fork_with_confidence(0.1),
            None
        );
        // Neither slot meets the minimum level of confidence 0.6 at depth 1
        assert_eq!(
            fork_confidence_cache.get_fork_with_depth_confidence(1, 0.6),
            None
        );
        // Only slot 0 meets the minimum level of confidence 0.5 at depth 1
        assert_eq!(
            fork_confidence_cache.get_fork_with_depth_confidence(1, 0.5),
            Some(0)
        );
        // If multiple slots meet the minimum level of confidence, method should return the most recent
        assert_eq!(
            fork_confidence_cache.get_fork_with_depth_confidence(1, 0.4),
            Some(1)
        );
        // If multiple slots meet the minimum level of confidence, method should return the most recent
        assert_eq!(
            fork_confidence_cache.get_fork_with_depth_confidence(0, 0.6),
            Some(1)
        );
        // Neither slot meets the minimum level of confidence 0.9 at depth 0
        assert_eq!(
            fork_confidence_cache.get_fork_with_depth_confidence(0, 0.9),
            None
        );
    }

    #[test]
    fn test_get_rooted_fork_with_confidence() {
        // Build ForkConfidenceCache with rooted votes
        let mut cache0 = BankConfidence::new([0; MAX_LOCKOUT_HISTORY]);
        cache0.increase_confirmation_stake(MAX_LOCKOUT_HISTORY, 40);
        cache0.increase_confirmation_stake(MAX_LOCKOUT_HISTORY - 1, 10);
        let mut cache1 = BankConfidence::new([0; MAX_LOCKOUT_HISTORY]);
        cache1.increase_confirmation_stake(MAX_LOCKOUT_HISTORY, 30);
        cache1.increase_confirmation_stake(MAX_LOCKOUT_HISTORY - 1, 10);
        cache1.increase_confirmation_stake(MAX_LOCKOUT_HISTORY - 2, 10);

        let mut bank_confidence = HashMap::new();
        bank_confidence.entry(0).or_insert(cache0.clone());
        bank_confidence.entry(1).or_insert(cache1.clone());
        let fork_confidence_cache = ForkConfidenceCache::new(bank_confidence, 50);

        // Only slot 0 meets the minimum level of confidence 0.66 at root
        assert_eq!(
            fork_confidence_cache.get_rooted_fork_with_confidence(0.66),
            Some(0)
        );
        // If multiple slots meet the minimum level of confidence, method should return the most recent
        assert_eq!(
            fork_confidence_cache.get_rooted_fork_with_confidence(0.6),
            Some(1)
        );
        // Neither slot meets the minimum level of confidence 0.9 at root
        assert_eq!(
            fork_confidence_cache.get_rooted_fork_with_confidence(0.9),
            None
        );
    }

    #[test]
    fn test_aggregate_confidence_for_vote_account_1() {
        let ancestors = vec![3, 4, 5, 7, 9, 11];
        let mut confidence = HashMap::new();
        let lamports = 5;
        let mut vote_state = VoteState::default();

        let root = ancestors.last().unwrap();
        vote_state.root_slot = Some(*root);
        AggregateConfidenceService::aggregate_confidence_for_vote_account(
            &mut confidence,
            &vote_state,
            &ancestors,
            lamports,
        );

        for a in ancestors {
            let mut expected = BankConfidence::default();
            expected.increase_confirmation_stake(MAX_LOCKOUT_HISTORY, lamports);
            assert_eq!(*confidence.get(&a).unwrap(), expected);
        }
    }

    #[test]
    fn test_aggregate_confidence_for_vote_account_2() {
        let ancestors = vec![3, 4, 5, 7, 9, 11];
        let mut confidence = HashMap::new();
        let lamports = 5;
        let mut vote_state = VoteState::default();

        let root = ancestors[2];
        vote_state.root_slot = Some(root);
        vote_state.process_slot_vote_unchecked(*ancestors.last().unwrap());
        AggregateConfidenceService::aggregate_confidence_for_vote_account(
            &mut confidence,
            &vote_state,
            &ancestors,
            lamports,
        );

        for a in ancestors {
            if a <= root {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(MAX_LOCKOUT_HISTORY, lamports);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            } else {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(1, lamports);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            }
        }
    }

    #[test]
    fn test_aggregate_confidence_for_vote_account_3() {
        let ancestors = vec![3, 4, 5, 7, 9, 10, 11];
        let mut confidence = HashMap::new();
        let lamports = 5;
        let mut vote_state = VoteState::default();

        let root = ancestors[2];
        vote_state.root_slot = Some(root);
        assert!(ancestors[4] + 2 >= ancestors[6]);
        vote_state.process_slot_vote_unchecked(ancestors[4]);
        vote_state.process_slot_vote_unchecked(ancestors[6]);
        AggregateConfidenceService::aggregate_confidence_for_vote_account(
            &mut confidence,
            &vote_state,
            &ancestors,
            lamports,
        );

        for (i, a) in ancestors.iter().enumerate() {
            if *a <= root {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(MAX_LOCKOUT_HISTORY, lamports);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            } else if i <= 4 {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(2, lamports);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            } else if i <= 6 {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(1, lamports);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            }
        }
    }

    #[test]
    fn test_aggregate_confidence_validity() {
        let ancestors = vec![3, 4, 5, 7, 9, 10, 11];
        let GenesisBlockInfo {
            mut genesis_block, ..
        } = create_genesis_block(10_000);

        let sk1 = Pubkey::new_rand();
        let pk1 = Pubkey::new_rand();
        let mut vote_account1 = vote_state::create_account(&pk1, &Pubkey::new_rand(), 0, 100);
        let stake_account1 = stake_state::create_account(&sk1, &pk1, &vote_account1, 100);
        let sk2 = Pubkey::new_rand();
        let pk2 = Pubkey::new_rand();
        let mut vote_account2 = vote_state::create_account(&pk2, &Pubkey::new_rand(), 0, 50);
        let stake_account2 = stake_state::create_account(&sk2, &pk2, &vote_account2, 50);

        genesis_block.accounts.extend(vec![
            (pk1, vote_account1.clone()),
            (sk1, stake_account1),
            (pk2, vote_account2.clone()),
            (sk2, stake_account2),
        ]);

        // Create bank
        let bank = Arc::new(Bank::new(&genesis_block));

        let mut vote_state1 = VoteState::from(&vote_account1).unwrap();
        vote_state1.process_slot_vote_unchecked(3);
        vote_state1.process_slot_vote_unchecked(5);
        vote_state1.to(&mut vote_account1).unwrap();
        bank.store_account(&pk1, &vote_account1);

        let mut vote_state2 = VoteState::from(&vote_account2).unwrap();
        vote_state2.process_slot_vote_unchecked(9);
        vote_state2.process_slot_vote_unchecked(10);
        vote_state2.to(&mut vote_account2).unwrap();
        bank.store_account(&pk2, &vote_account2);

        let confidence = AggregateConfidenceService::aggregate_confidence(&ancestors, &bank);

        for a in ancestors {
            if a <= 3 {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(2, 150);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            } else if a <= 5 {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(1, 100);
                expected.increase_confirmation_stake(2, 50);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            } else if a <= 9 {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(2, 50);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            } else if a <= 10 {
                let mut expected = BankConfidence::default();
                expected.increase_confirmation_stake(1, 50);
                assert_eq!(*confidence.get(&a).unwrap(), expected);
            } else {
                assert!(confidence.get(&a).is_none());
            }
        }
    }
}
