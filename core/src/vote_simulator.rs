use {
    crate::{
        cluster_info_vote_listener::VoteTracker,
        cluster_slots_service::cluster_slots::ClusterSlots,
        consensus::{
            fork_choice::SelectVoteAndResetForkResult,
            heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
            latest_validator_votes_for_frozen_banks::LatestValidatorVotesForFrozenBanks,
            progress_map::{ForkProgress, ProgressMap},
            Tower,
        },
        repair::cluster_slot_state_verifier::{
            DuplicateSlotsTracker, EpochSlotsFrozenSlots, GossipDuplicateConfirmedSlots,
        },
        replay_stage::{HeaviestForkFailures, ReplayStage},
        unfrozen_gossip_verified_vote_hashes::UnfrozenGossipVerifiedVoteHashes,
    },
    crossbeam_channel::unbounded,
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
            bank_forks,
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
            let new_bank = Bank::new_from_parent(parent_bank.clone(), &Pubkey::default(), slot);
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

        let new_root = tower.record_bank_vote(&vote_bank);
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

    #[allow(clippy::type_complexity)]
    fn init_state(
        num_keypairs: usize,
    ) -> (
        HashMap<Pubkey, ValidatorVoteKeypairs>,
        Vec<Pubkey>,
        Vec<Pubkey>,
        Arc<RwLock<BankForks>>,
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
) -> (
    Arc<RwLock<BankForks>>,
    ProgressMap,
    HeaviestSubtreeForkChoice,
) {
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
    let bank_forks = BankForks::new_rw_arc(bank0);
    let heaviest_subtree_fork_choice =
        HeaviestSubtreeForkChoice::new_from_bank_forks(bank_forks.clone());
    (bank_forks, progress, heaviest_subtree_fork_choice)
}
