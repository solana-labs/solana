use {
    crate::{
        cluster_info_vote_listener::VoteTracker,
        cluster_slot_state_verifier::{
            DuplicateSlotsTracker, EpochSlotsFrozenSlots, GossipDuplicateConfirmedSlots,
        },
        cluster_slots::ClusterSlots,
        replay_stage::ReplayStage,
        unfrozen_gossip_verified_vote_hashes::UnfrozenGossipVerifiedVoteHashes,
    },
    crossbeam_channel::unbounded,
    solana_consensus::{
        consensus::Tower,
        fork_choice::{HeaviestForkFailures, SelectVoteAndResetForkResult},
        heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
        latest_validator_votes_for_frozen_banks::LatestValidatorVotesForFrozenBanks,
        progress_map::{ForkProgress, ProgressMap},
    },
    solana_runtime::{
        accounts_background_service::AbsRequestSender,
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{
            create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
        },
    },
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey, signature::Signer},
    solana_vote_program::vote_transaction,
    std::{
        collections::{HashMap, HashSet},
        sync::{Arc, RwLock},
    },
    trees::{tr, Tree, TreeWalk},
};

pub struct VoteSimulator {
    pub validator_keypairs: HashMap<Pubkey, ValidatorVoteKeypairs>,
    pub node_pubkeys: Vec<Pubkey>,
    pub vote_pubkeys: Vec<Pubkey>,
    pub bank_forks: Arc<RwLock<BankForks>>,
    pub progress: ProgressMap,
    pub heaviest_subtree_fork_choice: HeaviestSubtreeForkChoice,
    pub latest_validator_votes_for_frozen_banks: LatestValidatorVotesForFrozenBanks,
}

impl VoteSimulator {
    pub fn new(num_keypairs: usize) -> Self {
        let (
            validator_keypairs,
            node_pubkeys,
            vote_pubkeys,
            bank_forks,
            progress,
            heaviest_subtree_fork_choice,
        ) = Self::init_state(num_keypairs);
        Self {
            validator_keypairs,
            node_pubkeys,
            vote_pubkeys,
            bank_forks: Arc::new(RwLock::new(bank_forks)),
            progress,
            heaviest_subtree_fork_choice,
            latest_validator_votes_for_frozen_banks: LatestValidatorVotesForFrozenBanks::default(),
        }
    }
    pub fn fill_bank_forks(
        &mut self,
        forks: Tree<u64>,
        cluster_votes: &HashMap<Pubkey, Vec<u64>>,
        is_frozen: bool,
    ) {
        let root = *forks.root().data();
        assert!(self.bank_forks.read().unwrap().get(root).is_some());

        let mut walk = TreeWalk::from(forks);

        while let Some(visit) = walk.get() {
            let slot = *visit.node().data();
            if self.bank_forks.read().unwrap().get(slot).is_some() {
                walk.forward();
                continue;
            }
            let parent = *walk.get_parent().unwrap().data();
            let parent_bank = self.bank_forks.read().unwrap().get(parent).unwrap();
            let new_bank = Bank::new_from_parent(&parent_bank, &Pubkey::default(), slot);
            self.progress
                .entry(slot)
                .or_insert_with(|| ForkProgress::new(Hash::default(), None, None, 0, 0));
            for (pubkey, vote) in cluster_votes.iter() {
                if vote.contains(&parent) {
                    let keypairs = self.validator_keypairs.get(pubkey).unwrap();
                    let latest_blockhash = parent_bank.last_blockhash();
                    let vote_tx = vote_transaction::new_vote_transaction(
                        // Must vote > root to be processed
                        vec![parent],
                        parent_bank.hash(),
                        latest_blockhash,
                        &keypairs.node_keypair,
                        &keypairs.vote_keypair,
                        &keypairs.vote_keypair,
                        None,
                    );
                    info!("voting {} {}", parent_bank.slot(), parent_bank.hash());
                    new_bank.process_transaction(&vote_tx).unwrap();

                    // Check the vote landed
                    let vote_account = new_bank
                        .get_vote_account(&keypairs.vote_keypair.pubkey())
                        .unwrap();
                    let state = vote_account.vote_state();
                    assert!(state
                        .as_ref()
                        .unwrap()
                        .votes
                        .iter()
                        .any(|lockout| lockout.slot() == parent));
                }
            }
            while new_bank.tick_height() < new_bank.max_tick_height() {
                new_bank.register_tick(&Hash::new_unique());
            }
            if !visit.node().has_no_child() || is_frozen {
                new_bank.freeze();
                self.progress
                    .get_fork_stats_mut(new_bank.slot())
                    .expect("All frozen banks must exist in the Progress map")
                    .bank_hash = Some(new_bank.hash());
                self.heaviest_subtree_fork_choice.add_new_leaf_slot(
                    (new_bank.slot(), new_bank.hash()),
                    Some((new_bank.parent_slot(), new_bank.parent_hash())),
                );
            }
            self.bank_forks.write().unwrap().insert(new_bank);

            walk.forward();
        }
    }

    pub fn simulate_vote(
        &mut self,
        vote_slot: Slot,
        my_pubkey: &Pubkey,
        tower: &mut Tower,
    ) -> Vec<HeaviestForkFailures> {
        // Try to simulate the vote
        let my_keypairs = self.validator_keypairs.get(my_pubkey).unwrap();
        let my_vote_pubkey = my_keypairs.vote_keypair.pubkey();
        let ancestors = self.bank_forks.read().unwrap().ancestors();
        let mut frozen_banks: Vec<_> = self
            .bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();

        let _ = ReplayStage::compute_bank_stats(
            my_pubkey,
            &ancestors,
            &mut frozen_banks,
            tower,
            &mut self.progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &self.bank_forks,
            &mut self.heaviest_subtree_fork_choice,
            &mut self.latest_validator_votes_for_frozen_banks,
        );

        let vote_bank = self
            .bank_forks
            .read()
            .unwrap()
            .get(vote_slot)
            .expect("Bank must have been created before vote simulation");

        // Try to vote on the given slot
        let descendants = self.bank_forks.read().unwrap().descendants();
        let SelectVoteAndResetForkResult {
            heaviest_fork_failures,
            ..
        } = ReplayStage::select_vote_and_reset_forks(
            &vote_bank,
            None,
            &ancestors,
            &descendants,
            &self.progress,
            tower,
            &self.latest_validator_votes_for_frozen_banks,
            &self.heaviest_subtree_fork_choice,
        );

        // Make sure this slot isn't locked out or failing threshold
        info!("Checking vote: {}", vote_bank.slot());
        if !heaviest_fork_failures.is_empty() {
            return heaviest_fork_failures;
        }

        let new_root = tower.record_bank_vote(&vote_bank, &my_vote_pubkey);
        if let Some(new_root) = new_root {
            self.set_root(new_root);
        }

        vec![]
    }

    pub fn set_root(&mut self, new_root: Slot) {
        let (drop_bank_sender, _drop_bank_receiver) = unbounded();
        ReplayStage::handle_new_root(
            new_root,
            &self.bank_forks,
            &mut self.progress,
            &AbsRequestSender::default(),
            None,
            &mut self.heaviest_subtree_fork_choice,
            &mut DuplicateSlotsTracker::default(),
            &mut GossipDuplicateConfirmedSlots::default(),
            &mut UnfrozenGossipVerifiedVoteHashes::default(),
            &mut true,
            &mut Vec::new(),
            &mut EpochSlotsFrozenSlots::default(),
            &drop_bank_sender,
        )
    }

    pub fn create_and_vote_new_branch(
        &mut self,
        start_slot: Slot,
        end_slot: Slot,
        cluster_votes: &HashMap<Pubkey, Vec<u64>>,
        votes_to_simulate: &HashSet<Slot>,
        my_pubkey: &Pubkey,
        tower: &mut Tower,
    ) -> HashMap<Slot, Vec<HeaviestForkFailures>> {
        (start_slot + 1..=end_slot)
            .filter_map(|slot| {
                let mut fork_tip_parent = tr(slot - 1);
                fork_tip_parent.push_front(tr(slot));
                self.fill_bank_forks(fork_tip_parent, cluster_votes, true);
                if votes_to_simulate.contains(&slot) {
                    Some((slot, self.simulate_vote(slot, my_pubkey, tower)))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn simulate_lockout_interval(
        &mut self,
        slot: Slot,
        lockout_interval: (u64, u64),
        vote_account_pubkey: &Pubkey,
    ) {
        self.progress
            .entry(slot)
            .or_insert_with(|| ForkProgress::new(Hash::default(), None, None, 0, 0))
            .fork_stats
            .lockout_intervals
            .entry(lockout_interval.1)
            .or_default()
            .push((lockout_interval.0, *vote_account_pubkey));
    }

    pub fn can_progress_on_fork(
        &mut self,
        my_pubkey: &Pubkey,
        tower: &mut Tower,
        start_slot: u64,
        num_slots: u64,
        cluster_votes: &mut HashMap<Pubkey, Vec<u64>>,
    ) -> bool {
        // Check that within some reasonable time, validator can make a new
        // root on this fork
        let old_root = tower.root();

        for i in 1..num_slots {
            // The parent of the tip of the fork
            let mut fork_tip_parent = tr(start_slot + i - 1);
            // The tip of the fork
            fork_tip_parent.push_front(tr(start_slot + i));
            self.fill_bank_forks(fork_tip_parent, cluster_votes, true);
            if self
                .simulate_vote(i + start_slot, my_pubkey, tower)
                .is_empty()
            {
                cluster_votes
                    .entry(*my_pubkey)
                    .or_default()
                    .push(start_slot + i);
            }
            if old_root != tower.root() {
                return true;
            }
        }

        false
    }

    fn init_state(
        num_keypairs: usize,
    ) -> (
        HashMap<Pubkey, ValidatorVoteKeypairs>,
        Vec<Pubkey>,
        Vec<Pubkey>,
        BankForks,
        ProgressMap,
        HeaviestSubtreeForkChoice,
    ) {
        let keypairs: HashMap<_, _> = std::iter::repeat_with(|| {
            let vote_keypairs = ValidatorVoteKeypairs::new_rand();
            (vote_keypairs.node_keypair.pubkey(), vote_keypairs)
        })
        .take(num_keypairs)
        .collect();
        let node_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.node_keypair.pubkey())
            .collect();
        let vote_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.vote_keypair.pubkey())
            .collect();

        let (bank_forks, progress, heaviest_subtree_fork_choice) =
            initialize_state(&keypairs, 10_000);
        (
            keypairs,
            node_pubkeys,
            vote_pubkeys,
            bank_forks,
            progress,
            heaviest_subtree_fork_choice,
        )
    }
}

// Setup BankForks with bank 0 and all the validator accounts
pub fn initialize_state(
    validator_keypairs_map: &HashMap<Pubkey, ValidatorVoteKeypairs>,
    stake: u64,
) -> (BankForks, ProgressMap, HeaviestSubtreeForkChoice) {
    let validator_keypairs: Vec<_> = validator_keypairs_map.values().collect();
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_vote_accounts(
        1_000_000_000,
        &validator_keypairs,
        vec![stake; validator_keypairs.len()],
    );

    genesis_config.poh_config.hashes_per_tick = Some(2);
    let bank0 = Bank::new_for_tests(&genesis_config);

    for pubkey in validator_keypairs_map.keys() {
        bank0.transfer(10_000, &mint_keypair, pubkey).unwrap();
    }

    while bank0.tick_height() < bank0.max_tick_height() {
        bank0.register_tick(&Hash::new_unique());
    }
    bank0.freeze();
    let mut progress = ProgressMap::default();
    progress.insert(
        0,
        ForkProgress::new_from_bank(&bank0, bank0.collector_id(), &Pubkey::default(), None, 0, 0),
    );
    let bank_forks = BankForks::new(bank0);
    let heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_bank_forks(&bank_forks);
    (bank_forks, progress, heaviest_subtree_fork_choice)
}

pub mod test {
    use {
        super::*,
        solana_consensus::{
            consensus::SwitchForkDecision,
            fork_choice::ForkChoice,
            tree_diff::TreeDiff,
        },
        solana_sdk::sysvar::slot_history::SlotHistory,
        itertools::Itertools,
    };

    #[test]
    fn test_simple_votes() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(1);
        let node_pubkey = vote_simulator.node_pubkeys[0];
        let mut tower = Tower::default();

        // Create the tree of banks
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3) / (tr(4) / tr(5)))));

        // Set the voting behavior
        let mut cluster_votes = HashMap::new();
        let votes = vec![1, 2, 3, 4, 5];
        cluster_votes.insert(node_pubkey, votes.clone());
        vote_simulator.fill_bank_forks(forks, &cluster_votes, true);

        // Simulate the votes
        for vote in votes {
            assert!(vote_simulator
                .simulate_vote(vote, &node_pubkey, &mut tower,)
                .is_empty());
        }

        for i in 1..5 {
            assert_eq!(tower.vote_state.votes[i - 1].slot() as usize, i);
            assert_eq!(
                tower.vote_state.votes[i - 1].confirmation_count() as usize,
                6 - i
            );
        }
    }

    #[test]
    fn test_switch_threshold_duplicate_rollback() {
        run_test_switch_threshold_duplicate_rollback(false);
    }

    #[test]
    #[should_panic]
    fn test_switch_threshold_duplicate_rollback_panic() {
        run_test_switch_threshold_duplicate_rollback(true);
    }

    fn setup_switch_test(num_accounts: usize) -> (Arc<Bank>, VoteSimulator, u64) {
        // Init state
        assert!(num_accounts > 1);
        let mut vote_simulator = VoteSimulator::new(num_accounts);
        let bank0 = vote_simulator.bank_forks.read().unwrap().get(0).unwrap();
        let total_stake = bank0.total_epoch_stake();
        assert_eq!(
            total_stake,
            vote_simulator.validator_keypairs.len() as u64 * 10_000
        );

        // Create the tree of banks
        let forks = tr(0)
            / (tr(1)
                / (tr(2)
                    // Minor fork 1
                    / (tr(10) / (tr(11) / (tr(12) / (tr(13) / (tr(14))))))
                    / (tr(43)
                        / (tr(44)
                            // Minor fork 2
                            / (tr(45) / (tr(46) / (tr(47) / (tr(48) / (tr(49) / (tr(50)))))))
                            / (tr(110)))
                        / tr(112))));

        // Fill the BankForks according to the above fork structure
        vote_simulator.fill_bank_forks(forks, &HashMap::new(), true);
        for (_, fork_progress) in vote_simulator.progress.iter_mut() {
            fork_progress.fork_stats.computed = true;
        }

        (bank0, vote_simulator, total_stake)
    }

    fn run_test_switch_threshold_duplicate_rollback(should_panic: bool) {
        let (bank0, mut vote_simulator, total_stake) = setup_switch_test(2);
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        let mut tower = Tower::default();

        // Last vote is 47
        tower.record_vote(
            47,
            vote_simulator
                .bank_forks
                .read()
                .unwrap()
                .get(47)
                .unwrap()
                .hash(),
        );

        // Trying to switch to an ancestor of last vote should only not panic
        // if the current vote has a duplicate ancestor
        let ancestor_of_voted_slot = 43;
        let duplicate_ancestor1 = 44;
        let duplicate_ancestor2 = 45;
        vote_simulator
            .heaviest_subtree_fork_choice
            .mark_fork_invalid_candidate(&(
                duplicate_ancestor1,
                vote_simulator
                    .bank_forks
                    .read()
                    .unwrap()
                    .get(duplicate_ancestor1)
                    .unwrap()
                    .hash(),
            ));
        vote_simulator
            .heaviest_subtree_fork_choice
            .mark_fork_invalid_candidate(&(
                duplicate_ancestor2,
                vote_simulator
                    .bank_forks
                    .read()
                    .unwrap()
                    .get(duplicate_ancestor2)
                    .unwrap()
                    .hash(),
            ));
        assert_eq!(
            tower.check_switch_threshold(
                ancestor_of_voted_slot,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchDuplicateRollback(duplicate_ancestor2)
        );
        let mut confirm_ancestors = vec![duplicate_ancestor1];
        if should_panic {
            // Adding the last duplicate ancestor will
            // 1) Cause loop below to confirm last ancestor
            // 2) Check switch threshold on a vote ancestor when there
            // are no duplicates on that fork, which will cause a panic
            confirm_ancestors.push(duplicate_ancestor2);
        }
        for (i, duplicate_ancestor) in confirm_ancestors.into_iter().enumerate() {
            vote_simulator
                .heaviest_subtree_fork_choice
                .mark_fork_valid_candidate(&(
                    duplicate_ancestor,
                    vote_simulator
                        .bank_forks
                        .read()
                        .unwrap()
                        .get(duplicate_ancestor)
                        .unwrap()
                        .hash(),
                ));
            let res = tower.check_switch_threshold(
                ancestor_of_voted_slot,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            );
            if i == 0 {
                assert_eq!(
                    res,
                    SwitchForkDecision::FailedSwitchDuplicateRollback(duplicate_ancestor2)
                );
            }
        }
    }

    #[test]
    fn test_switch_threshold() {
        let (bank0, mut vote_simulator, total_stake) = setup_switch_test(2);
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let mut descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        let mut tower = Tower::default();
        let other_vote_account = vote_simulator.vote_pubkeys[1];

        // Last vote is 47
        tower.record_vote(47, Hash::default());

        // Trying to switch to a descendant of last vote should always work
        assert_eq!(
            tower.check_switch_threshold(
                48,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::SameFork
        );

        // Trying to switch to another fork at 110 should fail
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Adding another validator lockout on a descendant of last vote should
        // not count toward the switch threshold
        vote_simulator.simulate_lockout_interval(50, (49, 100), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Adding another validator lockout on an ancestor of last vote should
        // not count toward the switch threshold
        vote_simulator.simulate_lockout_interval(50, (45, 100), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Adding another validator lockout on a different fork, but the lockout
        // doesn't cover the last vote, should not satisfy the switch threshold
        vote_simulator.simulate_lockout_interval(14, (12, 46), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Adding another validator lockout on a different fork, and the lockout
        // covers the last vote would count towards the switch threshold,
        // unless the bank is not the most recent frozen bank on the fork (14 is a
        // frozen/computed bank > 13 on the same fork in this case)
        vote_simulator.simulate_lockout_interval(13, (12, 47), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Adding another validator lockout on a different fork, and the lockout
        // covers the last vote, should satisfy the switch threshold
        vote_simulator.simulate_lockout_interval(14, (12, 47), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        // Adding another unfrozen descendant of the tip of 14 should not remove
        // slot 14 from consideration because it is still the most recent frozen
        // bank on its fork
        descendants.get_mut(&14).unwrap().insert(10000);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        // If we set a root, then any lockout intervals below the root shouldn't
        // count toward the switch threshold. This means the other validator's
        // vote lockout no longer counts
        tower.vote_state.root_slot = Some(43);
        // Refresh ancestors and descendants for new root.
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();

        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );
    }

    #[test]
    fn test_switch_threshold_use_gossip_votes() {
        let num_validators = 2;
        let (bank0, mut vote_simulator, total_stake) = setup_switch_test(2);
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        let mut tower = Tower::default();
        let other_vote_account = vote_simulator.vote_pubkeys[1];

        // Last vote is 47
        tower.record_vote(47, Hash::default());

        // Trying to switch to another fork at 110 should fail
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, num_validators * 10000)
        );

        // Adding a vote on the descendant shouldn't count toward the switch threshold
        vote_simulator.simulate_lockout_interval(50, (49, 100), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Adding a later vote from gossip that isn't on the same fork should count toward the
        // switch threshold
        vote_simulator
            .latest_validator_votes_for_frozen_banks
            .check_add_vote(
                other_vote_account,
                112,
                Some(
                    vote_simulator
                        .bank_forks
                        .read()
                        .unwrap()
                        .get(112)
                        .unwrap()
                        .hash(),
                ),
                false,
            );

        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        // If we now set a root that causes slot 112 to be purged from BankForks, then
        // the switch proof will now fail since that validator's vote can no longer be
        // included in the switching proof
        vote_simulator.set_root(44);
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );
    }

    #[test]
    fn test_switch_threshold_votes() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(4);
        let node_pubkey = vote_simulator.node_pubkeys[0];
        let mut tower = Tower::default();
        let forks = tr(0)
            / (tr(1)
                / (tr(2)
                    // Minor fork 1
                    / (tr(10) / (tr(11) / (tr(12) / (tr(13) / (tr(14))))))
                    / (tr(43)
                        / (tr(44)
                            // Minor fork 2
                            / (tr(45) / (tr(46))))
                        / (tr(110)))));

        // Have two validators, each representing 20% of the stake vote on
        // minor fork 2 at slots 46 + 47
        let mut cluster_votes: HashMap<Pubkey, Vec<Slot>> = HashMap::new();
        cluster_votes.insert(vote_simulator.node_pubkeys[1], vec![46]);
        cluster_votes.insert(vote_simulator.node_pubkeys[2], vec![47]);
        vote_simulator.fill_bank_forks(forks, &cluster_votes, true);

        // Vote on the first minor fork at slot 14, should succeed
        assert!(vote_simulator
            .simulate_vote(14, &node_pubkey, &mut tower,)
            .is_empty());

        // The other two validators voted at slots 46, 47, which
        // will only both show up in slot 48, at which point
        // 2/5 > SWITCH_FORK_THRESHOLD of the stake has voted
        // on another fork, so switching should succeed
        let votes_to_simulate = (46..=48).collect();
        let results = vote_simulator.create_and_vote_new_branch(
            45,
            48,
            &cluster_votes,
            &votes_to_simulate,
            &node_pubkey,
            &mut tower,
        );
        assert_eq!(
            *results.get(&46).unwrap(),
            vec![HeaviestForkFailures::FailedSwitchThreshold(46, 0, 40000)]
        );
        assert_eq!(
            *results.get(&47).unwrap(),
            vec![HeaviestForkFailures::FailedSwitchThreshold(
                47, 10000, 40000
            )]
        );
        assert!(results.get(&48).unwrap().is_empty());
    }

    #[test]
    fn test_double_partition() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(2);
        let node_pubkey = vote_simulator.node_pubkeys[0];
        let vote_pubkey = vote_simulator.vote_pubkeys[0];
        let mut tower = Tower::default();

        let num_slots_to_try = 200;
        // Create the tree of banks
        let forks = tr(0)
            / (tr(1)
                / (tr(2)
                    / (tr(3)
                        / (tr(4)
                            / (tr(5)
                                / (tr(6)
                                    / (tr(7)
                                        / (tr(8)
                                            / (tr(9)
                                                // Minor fork 1
                                                / (tr(10) / (tr(11) / (tr(12) / (tr(13) / (tr(14))))))
                                                / (tr(43)
                                                    / (tr(44)
                                                        // Minor fork 2
                                                        / (tr(45) / (tr(46) / (tr(47) / (tr(48) / (tr(49) / (tr(50)))))))
                                                        / (tr(110) / (tr(110 + 2 * num_slots_to_try))))))))))))));

        // Set the successful voting behavior
        let mut cluster_votes = HashMap::new();
        let mut my_votes: Vec<Slot> = vec![];
        let next_unlocked_slot = 110;
        // Vote on the first minor fork
        my_votes.extend(1..=14);
        // Come back to the main fork
        my_votes.extend(43..=44);
        // Vote on the second minor fork
        my_votes.extend(45..=50);
        // Vote to come back to main fork
        my_votes.push(next_unlocked_slot);
        cluster_votes.insert(node_pubkey, my_votes.clone());
        // Make the other validator vote fork to pass the threshold checks
        let other_votes = my_votes.clone();
        cluster_votes.insert(vote_simulator.node_pubkeys[1], other_votes);
        vote_simulator.fill_bank_forks(forks, &cluster_votes, true);

        // Simulate the votes.
        for vote in &my_votes {
            // All these votes should be ok
            assert!(vote_simulator
                .simulate_vote(*vote, &node_pubkey, &mut tower,)
                .is_empty());
        }

        info!("local tower: {:#?}", tower.vote_state.votes);
        let observed = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(next_unlocked_slot)
            .unwrap()
            .get_vote_account(&vote_pubkey)
            .unwrap();
        let state = observed.vote_state();
        info!("observed tower: {:#?}", state.as_ref().unwrap().votes);

        let num_slots_to_try = 200;
        cluster_votes
            .get_mut(&vote_simulator.node_pubkeys[1])
            .unwrap()
            .extend(next_unlocked_slot + 1..next_unlocked_slot + num_slots_to_try);
        assert!(vote_simulator.can_progress_on_fork(
            &node_pubkey,
            &mut tower,
            next_unlocked_slot,
            num_slots_to_try,
            &mut cluster_votes,
        ));
    }

    #[test]
    fn test_switch_threshold_across_tower_reload() {
        solana_logger::setup();
        // Init state
        let mut vote_simulator = VoteSimulator::new(2);
        let other_vote_account = vote_simulator.vote_pubkeys[1];
        let bank0 = vote_simulator.bank_forks.read().unwrap().get(0).unwrap();
        let total_stake = bank0.total_epoch_stake();
        assert_eq!(
            total_stake,
            vote_simulator.validator_keypairs.len() as u64 * 10_000
        );

        // Create the tree of banks
        let forks = tr(0)
            / (tr(1)
                / (tr(2)
                    / tr(10)
                    / (tr(43)
                        / (tr(44)
                            // Minor fork 2
                            / (tr(45) / (tr(46) / (tr(47) / (tr(48) / (tr(49) / (tr(50)))))))
                            / (tr(110) / tr(111))))));

        // Fill the BankForks according to the above fork structure
        vote_simulator.fill_bank_forks(forks, &HashMap::new(), true);
        for (_, fork_progress) in vote_simulator.progress.iter_mut() {
            fork_progress.fork_stats.computed = true;
        }

        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        let mut tower = Tower::default();

        tower.record_vote(43, Hash::default());
        tower.record_vote(44, Hash::default());
        tower.record_vote(45, Hash::default());
        tower.record_vote(46, Hash::default());
        tower.record_vote(47, Hash::default());
        tower.record_vote(48, Hash::default());
        tower.record_vote(49, Hash::default());

        // Trying to switch to a descendant of last vote should always work
        assert_eq!(
            tower.check_switch_threshold(
                50,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::SameFork
        );

        // Trying to switch to another fork at 110 should fail
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        vote_simulator.simulate_lockout_interval(111, (10, 49), &other_vote_account);

        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        assert_eq!(tower.voted_slots(), vec![43, 44, 45, 46, 47, 48, 49]);
        {
            let mut tower = tower.clone();
            tower.record_vote(110, Hash::default());
            tower.record_vote(111, Hash::default());
            assert_eq!(tower.voted_slots(), vec![43, 110, 111]);
            assert_eq!(tower.vote_state.root_slot, Some(0));
        }

        // Prepare simulated validator restart!
        let mut vote_simulator = VoteSimulator::new(2);
        let other_vote_account = vote_simulator.vote_pubkeys[1];
        let bank0 = vote_simulator.bank_forks.read().unwrap().get(0).unwrap();
        let total_stake = bank0.total_epoch_stake();
        let forks = tr(0)
            / (tr(1)
                / (tr(2)
                    / tr(10)
                    / (tr(43)
                        / (tr(44)
                            // Minor fork 2
                            / (tr(45) / (tr(46) / (tr(47) / (tr(48) / (tr(49) / (tr(50)))))))
                            / (tr(110) / tr(111))))));
        let replayed_root_slot = 44;

        // Fill the BankForks according to the above fork structure
        vote_simulator.fill_bank_forks(forks, &HashMap::new(), true);
        for (_, fork_progress) in vote_simulator.progress.iter_mut() {
            fork_progress.fork_stats.computed = true;
        }

        // prepend tower restart!
        let mut slot_history = SlotHistory::default();
        vote_simulator.set_root(replayed_root_slot);
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        for slot in &[0, 1, 2, 43, replayed_root_slot] {
            slot_history.add(*slot);
        }
        let mut tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![45, 46, 47, 48, 49]);

        // Trying to switch to another fork at 110 should fail
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Add lockout_interval which should be excluded
        vote_simulator.simulate_lockout_interval(111, (45, 50), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );

        // Add lockout_interval which should not be excluded
        vote_simulator.simulate_lockout_interval(111, (110, 200), &other_vote_account);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &ancestors,
                &descendants,
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
                &vote_simulator.latest_validator_votes_for_frozen_banks,
                &vote_simulator.heaviest_subtree_fork_choice,
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        tower.record_vote(110, Hash::default());
        tower.record_vote(111, Hash::default());
        assert_eq!(tower.voted_slots(), vec![110, 111]);
        assert_eq!(tower.vote_state.root_slot, Some(replayed_root_slot));
    }

    #[test]
    fn test_new_from_frozen_banks() {
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |    slot 3
            slot 4
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3)));
        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::new(), true);
        let bank_forks = vote_simulator.bank_forks;
        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        frozen_banks.sort_by_key(|bank| bank.slot());

        let root_bank = bank_forks.read().unwrap().root_bank();
        let root = root_bank.slot();
        let root_hash = root_bank.hash();
        let heaviest_subtree_fork_choice =
            HeaviestSubtreeForkChoice::new_from_frozen_banks((root, root_hash), &frozen_banks);

        let bank0_hash = bank_forks.read().unwrap().get(0).unwrap().hash();
        assert!(heaviest_subtree_fork_choice
            .parent(&(0, bank0_hash))
            .is_none());

        let bank1_hash = bank_forks.read().unwrap().get(1).unwrap().hash();
        assert_eq!(
            (&heaviest_subtree_fork_choice)
                .children(&(0, bank0_hash))
                .unwrap()
                .collect_vec(),
            &[&(1, bank1_hash)]
        );

        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(1, bank1_hash)),
            Some((0, bank0_hash))
        );
        let bank2_hash = bank_forks.read().unwrap().get(2).unwrap().hash();
        let bank3_hash = bank_forks.read().unwrap().get(3).unwrap().hash();
        assert_eq!(
            (&heaviest_subtree_fork_choice)
                .children(&(1, bank1_hash))
                .unwrap()
                .collect_vec(),
            &[&(2, bank2_hash), &(3, bank3_hash)]
        );
        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(2, bank2_hash)),
            Some((1, bank1_hash))
        );
        let bank4_hash = bank_forks.read().unwrap().get(4).unwrap().hash();
        assert_eq!(
            (&heaviest_subtree_fork_choice)
                .children(&(2, bank2_hash))
                .unwrap()
                .collect_vec(),
            &[&(4, bank4_hash)]
        );
        // Check parent and children of invalid hash don't exist
        let invalid_hash = Hash::new_unique();
        assert!((&heaviest_subtree_fork_choice)
            .children(&(2, invalid_hash))
            .is_none());
        assert!(heaviest_subtree_fork_choice
            .parent(&(2, invalid_hash))
            .is_none());

        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(3, bank3_hash)),
            Some((1, bank1_hash))
        );
        assert!((&heaviest_subtree_fork_choice)
            .children(&(3, bank3_hash))
            .unwrap()
            .collect_vec()
            .is_empty());
        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(4, bank4_hash)),
            Some((2, bank2_hash))
        );
        assert!((&heaviest_subtree_fork_choice)
            .children(&(4, bank4_hash))
            .unwrap()
            .collect_vec()
            .is_empty());
    }

}
