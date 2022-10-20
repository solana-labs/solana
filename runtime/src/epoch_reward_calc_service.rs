//! A service to calculate stake rewards at epoch boundary
//!

use {
    crate::{
        bank::{Bank, RewardsMetrics, StakeVoteAccountRewardResult},
        bank_forks::BankForks,
    },
    crossbeam_channel::{Receiver, Sender},
    log::*,
    rayon::ThreadPoolBuilder,
    solana_measure::{measure, measure::Measure},
    solana_sdk::{clock::Slot, hash::Hash, stake_history::Epoch},
    std::{
        collections::HashMap,
        fmt::Debug,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

/// Epoch reward calculation request with current epoch and target bank
#[derive(AbiExample, Debug)]
pub struct EpochRewardCalcRequest {
    /// current epoch
    epoch: Epoch,

    /// Bank
    bank: Arc<Bank>,
}

impl EpochRewardCalcRequest {
    /// Create epoch reward calculation request
    pub fn new(epoch: Epoch, bank: Arc<Bank>) -> Self {
        Self { epoch, bank }
    }
}

/// Sender of EpochRewardCalcRequest
pub type EpochRewardCalcRequestSender = Sender<EpochRewardCalcRequest>;

/// Receiver of EpochRewardCalcRequest
pub type EpochRewardCalcRequestReceiver = Receiver<EpochRewardCalcRequest>;

/// Epoch reward computation result indexed by slot and calc request signature
#[derive(AbiExample, Debug)]
pub struct EpochRewardResult<T> {
    /// Map from reward calculate request hash to reward result
    rewards: HashMap<Hash, Arc<T>>,

    /// Map from slot to reward calculation request hash
    signatures: HashMap<Slot, Hash>,

    /// Current epoch
    epoch: u64,
}

/// Default Trait
impl<T> Default for EpochRewardResult<T> {
    /// Default
    fn default() -> Self {
        Self::new()
    }
}

impl<T> EpochRewardResult<T> {
    /// Create
    pub fn new() -> Self {
        Self {
            rewards: HashMap::new(),
            signatures: HashMap::new(),
            epoch: 0,
        }
    }

    /// Get epoch result from slot
    pub fn get(&self, slot: Slot) -> Option<Arc<T>> {
        if let Some(signature) = self.signatures.get(&slot) {
            if let Some(result) = self.rewards.get(signature) {
                return Some(result.clone());
            }
        }
        None
    }

    /// Get number of rewards results
    pub fn rewards_len(&self) -> usize {
        self.rewards.len()
    }

    /// Get number of epoch reward calculation signatures
    pub fn signatures_len(&self) -> usize {
        self.signatures.len()
    }

    /// Clear epoch reward calculation results
    pub fn clear(&mut self) {
        self.rewards.clear();
        self.signatures.clear();
    }

    /// Clear old epoch results
    pub fn relinquish(&mut self, epoch: u64) {
        if epoch > self.epoch {
            self.clear();
            self.epoch = epoch;
        }
    }
}

/// Epoch reward calculator
///   A client class to send rewards calculation request and retrieve EpochRewardResult to/from
///   EpochRewardCalculationService
#[derive(AbiExample, Debug)]
pub struct EpochRewardCalculator<T> {
    /// Channel sender for epoch reward calculation request
    sender: EpochRewardCalcRequestSender,

    /// Epoch reward result
    results: Arc<RwLock<EpochRewardResult<T>>>,
}

impl<T> EpochRewardCalculator<T> {
    /// Create EpochRewardCalculator
    pub fn new(
        sender: EpochRewardCalcRequestSender,
        results: Arc<RwLock<EpochRewardResult<T>>>,
    ) -> Self {
        Self { sender, results }
    }

    /// Send epoch reward calculation request
    pub fn send(&self, epoch: Epoch, bank: Arc<Bank>) {
        let request = EpochRewardCalcRequest::new(epoch, bank);
        self.sender.send(request).unwrap();
    }

    /// Get epoch reward calculation result
    pub fn get(&self, slot: Slot) -> Option<Arc<T>> {
        self.results.read().unwrap().get(slot)
    }

    /// Clear epoch reward calculation result
    pub fn clear(&self) {
        self.results.write().unwrap().clear();
    }
}

/// Epoch rewards calculation metrics
#[derive(Debug, Default)]
pub(crate) struct RewardsCalcMetrics {
    epoch: Epoch,
    slot: Slot,
    parent_slot: Slot,
    load_calc_info_time_us: u64,
    calc_signature_time_us: u64,
    reward_calc_time_us: u64,
    total_time_us: u64,
}

impl RewardsCalcMetrics {
    pub fn report(self) {
        datapoint_info!(
            "handle_epoch_reward_calc_request_timings",
            ("epoch", self.epoch, i64),
            ("parent_slot", self.parent_slot, i64),
            ("slot", self.slot, i64),
            ("load_calc_info_us", self.load_calc_info_time_us, i64),
            ("calc_signature_us", self.calc_signature_time_us, i64),
            ("reward_calc_us", self.reward_calc_time_us, i64),
            ("total_us", self.total_time_us, i64),
        );
    }
}

/// EpochRewardCalcRequestHandler
///   Handler for incoming epoch reward calculation requests
pub struct EpochRewardCalcRequestHandler {
    /// Channel receiver for epoch reward calculation requests
    pub receiver: EpochRewardCalcRequestReceiver,

    /// Storage for epoch reward calculation results
    pub results: Arc<RwLock<EpochRewardResult<StakeVoteAccountRewardResult>>>,
}

impl EpochRewardCalcRequestHandler {
    /// Create handler
    pub fn new(
        receiver: EpochRewardCalcRequestReceiver,
        results: Arc<RwLock<EpochRewardResult<StakeVoteAccountRewardResult>>>,
    ) -> Self {
        Self { receiver, results }
    }

    /// Handle incoming calculation request
    pub fn handle_request(&self) {
        const MAX_REQUEST_RECV_TIME_MS: u64 = 500;
        if let Ok(request) = self
            .receiver
            .recv_timeout(Duration::from_millis(MAX_REQUEST_RECV_TIME_MS))
        {
            let EpochRewardCalcRequest { epoch, bank } = request;
            let parent_slot = bank.parent_slot();
            info!(
                "handle reward calculation request: epoch {} parent_slot {}",
                epoch, parent_slot
            );

            let mut calc_metrics = RewardsCalcMetrics {
                epoch: epoch,
                slot: bank.slot(),
                parent_slot: parent_slot,
                ..RewardsCalcMetrics::default()
            };

            self.results.write().unwrap().relinquish(epoch);

            if !self
                .results
                .read()
                .unwrap()
                .signatures
                .contains_key(&parent_slot)
            {
                let mut total_time = Measure::start("total");
                // hasn't seen the epoch boundary slot yet, load the stake_vote_delegation map and
                // compute the signature and add (boundary_slot, signature) to map.
                let thread_pool = ThreadPoolBuilder::new().build().unwrap();
                let mut metrics = RewardsMetrics::default();
                let (vote_with_stake_delegations_map, load_calc_info_time) =
                    measure!(bank.load_reward_calc_info(&thread_pool, &mut metrics));
                calc_metrics.load_calc_info_time_us = load_calc_info_time.as_us();

                let (signature, calc_signature_time) =
                    measure!(bank.compute_rewards_calc_signature(&vote_with_stake_delegations_map));
                calc_metrics.calc_signature_time_us = calc_signature_time.as_us();

                self.results
                    .write()
                    .unwrap()
                    .signatures
                    .insert(parent_slot, signature);

                if !self
                    .results
                    .read()
                    .unwrap()
                    .rewards
                    .contains_key(&signature)
                {
                    // hasn't seen the signature in the rewards yet, compute the rewards and save
                    // (signature, rewards) to the map.
                    let parent_epoch = epoch.saturating_sub(1); // TODO

                    let (result, reward_calc_time) = measure!(
                        bank.do_stake_reward_calc(
                            vote_with_stake_delegations_map,
                            parent_epoch,
                            &thread_pool,
                            &mut metrics,
                        ),
                        "stake_reward_calc",
                    );
                    self.results
                        .write()
                        .unwrap()
                        .rewards
                        .insert(signature, Arc::new(result));
                    calc_metrics.reward_calc_time_us = reward_calc_time.as_us();
                }

                total_time.stop();

                calc_metrics.total_time_us = total_time.as_us();
                calc_metrics.report();
            }
        }
    }
}

/// EpochRewardCalcService
///   A long running service to calculate epoch rewards
pub struct EpochRewardCalcService {
    /// Thread handler
    t_background: JoinHandle<()>,
}

impl EpochRewardCalcService {
    /// Create service
    pub fn new(handler: EpochRewardCalcRequestHandler, exit: &Arc<AtomicBool>) -> Self {
        info!("EpochRewardsCalcService active");
        let exit = exit.clone();
        let t_background = Builder::new()
            .name("solEpochRewardsCalcService".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                handler.handle_request();
            })
            .unwrap();
        Self { t_background }
    }

    /// Set up epoch_reward_calculator for banks
    /// Should be called immediately after bank_fork_utils::load_bank_forks(), and as such, there
    /// should only be one bank, the root bank, in `bank_forks`
    /// All banks added to `bank_forks` will be descended from the root bank, and thus will inherit
    /// the bank epoch reward calculator.
    pub fn setup_bank_epoch_reward_calculator(
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> (
        EpochRewardCalcRequestReceiver,
        Arc<RwLock<EpochRewardResult<StakeVoteAccountRewardResult>>>,
    ) {
        assert_eq!(bank_forks.read().unwrap().banks().len(), 1);

        let (epoch_reward_calc_sender, epoch_reward_calc_receiver) = crossbeam_channel::unbounded();
        let results = Arc::new(RwLock::new(EpochRewardResult::new()));

        {
            let root_bank = bank_forks.read().unwrap().root_bank();
            root_bank.set_epoch_reward_calculator(EpochRewardCalculator::<
                StakeVoteAccountRewardResult,
            >::new(
                epoch_reward_calc_sender, results.clone()
            ));
        }
        (epoch_reward_calc_receiver, results)
    }

    /// Handler thread join
    pub fn join(self) -> thread::Result<()> {
        self.t_background.join()
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{bank::VoteWithStakeDelegations, genesis_utils::create_genesis_config},
        dashmap::DashMap,
        solana_sdk::{account::AccountSharedData, epoch_schedule::EpochSchedule, pubkey::Pubkey},
        solana_vote_program::vote_state::VoteState,
        std::{default::Default, str::FromStr},
    };

    /// A test for the lifetime of epoch reward calculation service
    #[test]
    fn test_epoch_reward_service() {
        let exit = Arc::new(AtomicBool::new(false));

        let genesis = create_genesis_config(10);
        let bank0 = Bank::new_for_tests(&genesis.genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank0)));

        // setup the service
        let (epoch_reward_calc_receiver, epoch_reward_calc_results) =
            EpochRewardCalcService::setup_bank_epoch_reward_calculator(bank_forks.clone());

        assert_eq!(epoch_reward_calc_results.read().unwrap().rewards_len(), 0);
        assert_eq!(
            epoch_reward_calc_results.read().unwrap().signatures_len(),
            0
        );

        let epoch_reward_calc_request_handler = EpochRewardCalcRequestHandler::new(
            epoch_reward_calc_receiver,
            epoch_reward_calc_results,
        );

        let service = EpochRewardCalcService::new(epoch_reward_calc_request_handler, &exit);

        // add another bank
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);

        // send a request for reward calculation
        let reward_bank = bank_forks.read().unwrap().get(1).unwrap();
        let calc = reward_bank.get_epoch_reward_calculator();
        let inner = calc.read().unwrap();
        if let Some(calc) = &*inner {
            calc.send(reward_bank.epoch(), reward_bank.clone());
        }

        // shutdown service
        exit.store(true, Ordering::Relaxed);
        service.join().expect("epoch_reward_calc_service completed");
    }

    /// A test for epoch reward calculator class
    #[test]
    fn test_epoch_reward_calculator() {
        // set up
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let results = Arc::new(RwLock::new(EpochRewardResult::<i32>::new()));
        let calculator = EpochRewardCalculator::<i32>::new(sender, results.clone());

        // result is not available yet, assert none
        let slot = 100;
        assert_eq!(results.read().unwrap().signatures_len(), 0);
        assert_eq!(results.read().unwrap().rewards_len(), 0);
        assert!(calculator.get(slot).is_none());

        let handle_request_sim = |parent_slot: Slot, signature: Hash, val: i32| {
            if !results
                .read()
                .unwrap()
                .signatures
                .contains_key(&parent_slot)
            {
                results
                    .write()
                    .unwrap()
                    .signatures
                    .insert(parent_slot, signature);

                if !results.read().unwrap().rewards.contains_key(&signature) {
                    // hasn't seen the signature in the rewards yet, compute the rewards and save
                    // (signature, rewards) to the map.
                    results
                        .write()
                        .unwrap()
                        .rewards
                        .insert(signature, Arc::new(val));
                }
            }
        };

        // add 1st calculation, and assert correct value
        let sig1 = Hash::from_str("5K3NW73xFHwgTWVe4LyCg4QfQda8f88uZj2ypDx2kmmH").unwrap();
        handle_request_sim(slot, sig1, 1);
        assert_eq!(*(calculator.get(slot).unwrap()), 1);
        assert_eq!(results.read().unwrap().signatures_len(), 1);
        assert_eq!(results.read().unwrap().rewards_len(), 1);

        // add 2nd calculation from a fork slot with same signature, and assert correct value
        let slot2 = 101;
        handle_request_sim(slot2, sig1, 1);
        assert_eq!(*(calculator.get(slot2).unwrap()), 1);
        assert_eq!(results.read().unwrap().signatures_len(), 2);
        assert_eq!(results.read().unwrap().rewards_len(), 1);

        // add 3rd calculation from a fork slot with different signature, and assert correct value
        let slot3 = 102;
        let sig3 = Hash::from_str("4CCNp28j6AhGq7PkjPDP4wbQWBS8LLbQin2xV5n8frKX").unwrap();
        handle_request_sim(slot3, sig3, 2);
        assert_eq!(*(calculator.get(slot3).unwrap()), 2);
        assert_eq!(results.read().unwrap().signatures_len(), 3);
        assert_eq!(results.read().unwrap().rewards_len(), 2);

        // clear calculation results and assert none
        results.write().unwrap().clear();
        assert_eq!(results.read().unwrap().signatures_len(), 0);
        assert_eq!(results.read().unwrap().rewards_len(), 0);
        assert!(calculator.get(slot).is_none());
    }

    /// A test for compute the reward calculation signature
    #[test]
    fn test_compute_reward_calculation_signature() {
        let genesis = create_genesis_config(10);
        let bank0 = Bank::new_for_tests(&genesis.genesis_config);

        // set up vote accounts
        let mut vote_pubkeys = vec![];
        for _ in 0..5 {
            vote_pubkeys.push(Pubkey::new_unique());
        }

        // compute the first signature
        let vote_with_stake_delegations_map = DashMap::new();
        for vote_pubkey in vote_pubkeys.iter().copied() {
            let vote_account = AccountSharedData::new(264, 0, &solana_vote_program::id());
            bank0.store_account(&vote_pubkey, &vote_account);
            let vote_state = VoteState::default();
            let delegations = VoteWithStakeDelegations {
                vote_state: Arc::new(vote_state),
                vote_account,
                delegations: vec![],
            };
            vote_with_stake_delegations_map.insert(vote_pubkey, delegations);
        }
        let signature1 = bank0.compute_rewards_calc_signature(&vote_with_stake_delegations_map);

        // compute 2nd signature (in reverse order)
        let vote_with_stake_delegations_map2 = DashMap::new();
        for vote_pubkey in vote_pubkeys.iter().rev().copied() {
            let vote_account = AccountSharedData::new(264, 0, &solana_vote_program::id());
            bank0.store_account(&vote_pubkey, &vote_account);
            let vote_state = VoteState::default();
            let delegations = VoteWithStakeDelegations {
                vote_state: Arc::new(vote_state),
                vote_account,
                delegations: vec![],
            };
            vote_with_stake_delegations_map2.insert(vote_pubkey, delegations);
        }
        let signature2 = bank0.compute_rewards_calc_signature(&vote_with_stake_delegations_map);

        // assert
        assert_eq!(signature1, signature2);
    }

    /// A test for epoch service for longer than 1 epoch
    #[test]
    fn test_epoch_reward_service_long() {
        let exit = Arc::new(AtomicBool::new(false));

        let mut genesis = create_genesis_config(10);
        genesis.genesis_config.epoch_schedule = EpochSchedule::custom(32, 32, false);

        let bank0 = Bank::new_for_tests(&genesis.genesis_config);
        let reward_calculation_interval = bank0.get_reward_calculation_interval();
        let reward_credit_interval = bank0.get_reward_credit_interval();

        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank0)));

        // setup the service
        let (epoch_reward_calc_receiver, epoch_reward_calc_results) =
            EpochRewardCalcService::setup_bank_epoch_reward_calculator(bank_forks.clone());

        assert_eq!(epoch_reward_calc_results.read().unwrap().rewards_len(), 0);
        assert_eq!(
            epoch_reward_calc_results.read().unwrap().signatures_len(),
            0
        );

        let epoch_reward_calc_request_handler = EpochRewardCalcRequestHandler::new(
            epoch_reward_calc_receiver,
            epoch_reward_calc_results,
        );

        let service = EpochRewardCalcService::new(epoch_reward_calc_request_handler, &exit);

        let bank1 = Bank::new_from_parent(
            &(bank_forks.read().unwrap().get(0).unwrap()),
            &Pubkey::default(),
            1,
        );

        // first epoch
        let mut bank = Arc::new(bank1);
        let mut slot = 2;
        for _ in 0..30 {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            assert!(!bank.in_reward_interval());
            slot += 1;
        }

        // start new epoch and entering REWARD_CALCULATION_INTERVAL
        let mut reward_bank = None;
        let calc_start = 31;
        for _ in 0..reward_calculation_interval {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            if reward_bank.is_none() {
                reward_bank = Some(bank.clone());
            }
            assert!(bank.in_reward_interval());
            assert!(bank.in_reward_calc_interval());
            assert!(!bank.in_reward_redeem_interval());
            assert_eq!(bank.get_reward_progress_index().unwrap(), slot - calc_start);
            assert_eq!(bank.get_reward_elapsed_slots().unwrap(), slot - calc_start);
            slot += 1;
        }

        // In real validator, block store ledger would have initiated a reward calculation. And the
        // the result should be available by now. However, in this test, we don't set up the block
        // store.  Therefore, we simulate bank replay from block store by sending a request for
        // calculation from reward bank directly.
        let reward_bank = reward_bank.unwrap();
        let calc = reward_bank.get_epoch_reward_calculator();
        let inner = calc.read().unwrap();
        if let Some(calc) = &*inner {
            calc.send(reward_bank.epoch(), reward_bank.clone());
        }

        // entering REWARD_CREDIT_INTERVAL
        for _ in 0..reward_credit_interval {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            assert!(bank.in_reward_interval());
            assert!(!bank.in_reward_calc_interval());
            assert!(bank.in_reward_redeem_interval());

            assert_eq!(bank.get_reward_progress_index().unwrap(), slot - calc_start);
            assert_eq!(bank.get_reward_elapsed_slots().unwrap(), slot - calc_start);
            slot += 1;
        }

        // leave REWARD_CREDIT_INTERVAL, entering normal non-reward slots in the epoch
        for _ in 0..2 {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            assert!(!bank.in_reward_interval());
            slot += 1;
        }

        // shutdown the service
        exit.store(true, Ordering::Relaxed);
        service.join().expect("epoch_reward_calc_service completed");
    }

    /// A test for epoch reward calc progress
    #[test]
    fn test_epoch_reward_calc_progress() {
        let mut genesis = create_genesis_config(10);
        genesis.genesis_config.epoch_schedule = EpochSchedule::without_warmup();

        let bank0 = Arc::new(Bank::new_for_tests(&genesis.genesis_config));

        // set up (0, 0)
        let mut bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank1.set_epoch_reward_calc_start_for_test(0, 0);

        let reward_calculation_interval = bank0.get_reward_calculation_interval();
        let reward_credit_interval = bank0.get_reward_credit_interval();

        // assert bank1 - start calc bank (1, 1)
        assert!(bank1.in_reward_interval());
        assert!(bank1.in_reward_calc_interval());
        assert!(!bank1.in_reward_redeem_interval());
        assert_eq!(bank1.get_reward_progress_index().unwrap(), 1);
        assert_eq!(bank1.get_reward_elapsed_slots().unwrap(), 1);

        // assert calc interval [2..REWARD_CALCULATION_INTERVAL]
        let mut bank = Arc::new(bank1);
        for i in 0..reward_calculation_interval - 1 {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 2 + i));

            assert!(bank.in_reward_interval());
            assert!(bank.in_reward_calc_interval());
            assert!(!bank.in_reward_redeem_interval());
            assert_eq!(bank.get_reward_progress_index().unwrap(), 2 + i);
            assert_eq!(bank.get_reward_elapsed_slots().unwrap(), 2 + i);
        }

        // assert redeem interval [REWARD_CALCULATION_INTERVAL+1, REWARD_CALCULATION_INTERVAL+REWARD_CREDIT_INTERVAL]
        for i in 0..reward_credit_interval {
            bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                reward_calculation_interval + 1 + i,
            ));

            assert!(bank.in_reward_interval());
            assert!(!bank.in_reward_calc_interval());
            assert!(bank.in_reward_redeem_interval());
            assert_eq!(
                bank.get_reward_progress_index().unwrap(),
                reward_calculation_interval + 1 + i
            );
            assert_eq!(
                bank.get_reward_elapsed_slots().unwrap(),
                reward_calculation_interval + 1 + i
            );
        }

        // assert any following slots will not be in reward interval
        for i in 0..2 {
            bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                reward_calculation_interval + reward_credit_interval + 1 + i,
            ));

            assert!(!bank.in_reward_interval());
        }
    }

    /// A test for reward calculation at epoch boundary
    #[test]
    fn test_calc_start_cross_epoch() {
        let mut genesis = create_genesis_config(10);
        genesis.genesis_config.epoch_schedule = EpochSchedule::custom(32, 32, false);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis.genesis_config));

        let reward_calculation_interval = bank0.get_reward_calculation_interval();
        let reward_credit_interval = bank0.get_reward_credit_interval();

        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);

        // first epoch
        let mut bank = Arc::new(bank1);
        let mut slot = 2;
        for _ in 0..30 {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            assert!(!bank.in_reward_interval());
            slot += 1;
        }

        // start new epoch and entering REWARD_CALCULATION_INTERVAL
        let calc_start = 31;
        for _ in 0..reward_calculation_interval {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            assert!(bank.in_reward_interval());
            assert!(bank.in_reward_calc_interval());
            assert!(!bank.in_reward_redeem_interval());
            assert_eq!(bank.get_reward_progress_index().unwrap(), slot - calc_start);
            assert_eq!(bank.get_reward_elapsed_slots().unwrap(), slot - calc_start);
            slot += 1;
        }

        // entering REWARD_CREDIT_INTERVAL
        for _ in 0..reward_credit_interval {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            assert!(bank.in_reward_interval());
            assert!(!bank.in_reward_calc_interval());
            assert!(bank.in_reward_redeem_interval());

            assert_eq!(bank.get_reward_progress_index().unwrap(), slot - calc_start);
            assert_eq!(bank.get_reward_elapsed_slots().unwrap(), slot - calc_start);
            slot += 1;
        }

        // leaving REWARD_CREDIT_INTERVAL, entering normal non-reward slots in the epoch
        for _ in 0..2 {
            bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), slot));
            assert!(!bank.in_reward_interval());
            slot += 1;
        }
    }
}
