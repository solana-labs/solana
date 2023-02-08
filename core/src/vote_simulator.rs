use {
    crate::{
        cluster_info_vote_listener::VoteTracker,
        cluster_slot_state_verifier::{
            DuplicateSlotsTracker, EpochSlotsFrozenSlots, GossipDuplicateConfirmedSlots,
        },
        cluster_slots::ClusterSlots,
        consensus::Tower,
        fork_choice::SelectVoteAndResetForkResult,
        heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
        latest_validator_votes_for_frozen_banks::LatestValidatorVotesForFrozenBanks,
        progress_map::{ForkProgress, ProgressMap},
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
    ) 
    {
        panic!();
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
    panic!();
}

