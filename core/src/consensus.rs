pub mod fork_choice;
pub mod heaviest_subtree_fork_choice;
pub(crate) mod latest_validator_votes_for_frozen_banks;
pub mod progress_map;
mod tower1_14_11;
mod tower1_7_14;
pub mod tower_storage;
pub mod tree_diff;
pub mod vote_stake_tracker;

use {
    self::{
        heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
        latest_validator_votes_for_frozen_banks::LatestValidatorVotesForFrozenBanks,
        progress_map::{LockoutIntervals, ProgressMap},
        tower1_14_11::Tower1_14_11,
        tower1_7_14::Tower1_7_14,
        tower_storage::{SavedTower, SavedTowerVersions, TowerStorage},
    },
    chrono::prelude::*,
    solana_ledger::{ancestor_iterator::AncestorIterator, blockstore::Blockstore, blockstore_db},
    solana_runtime::{bank::Bank, bank_forks::BankForks, commitment::VOTE_THRESHOLD_SIZE},
    solana_sdk::{
        clock::{Slot, UnixTimestamp},
        hash::Hash,
        instruction::Instruction,
        pubkey::Pubkey,
        signature::Keypair,
        slot_history::{Check, SlotHistory},
    },
    solana_vote::vote_account::VoteAccountsHashMap,
    solana_vote_program::{
        vote_instruction,
        vote_state::{
            process_slot_vote_unchecked, process_vote_unchecked, BlockTimestamp, LandedVote,
            Lockout, Vote, VoteState, VoteState1_14_11, VoteStateUpdate, VoteStateVersions,
            VoteTransaction, MAX_LOCKOUT_HISTORY,
        },
    },
    std::{
        cmp::Ordering,
        collections::{HashMap, HashSet},
        ops::{
            Bound::{Included, Unbounded},
            Deref,
        },
    },
    thiserror::Error,
};

#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub enum ThresholdDecision {
    #[default]
    PassedThreshold,
    FailedThreshold(/* Observed stake */ u64),
}

impl ThresholdDecision {
    pub fn passed(&self) -> bool {
        matches!(self, Self::PassedThreshold)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, AbiExample)]
pub enum SwitchForkDecision {
    SwitchProof(Hash),
    SameFork,
    FailedSwitchThreshold(
        /* Switch proof stake */ u64,
        /* Total stake */ u64,
    ),
    FailedSwitchDuplicateRollback(Slot),
}

impl SwitchForkDecision {
    pub fn to_vote_instruction(
        &self,
        vote: VoteTransaction,
        vote_account_pubkey: &Pubkey,
        authorized_voter_pubkey: &Pubkey,
    ) -> Option<Instruction> {
        match (self, vote) {
            (SwitchForkDecision::FailedSwitchThreshold(_, total_stake), _) => {
                assert_ne!(*total_stake, 0);
                None
            }
            (SwitchForkDecision::FailedSwitchDuplicateRollback(_), _) => None,
            (SwitchForkDecision::SameFork, VoteTransaction::Vote(v)) => Some(
                vote_instruction::vote(vote_account_pubkey, authorized_voter_pubkey, v),
            ),
            (SwitchForkDecision::SameFork, VoteTransaction::VoteStateUpdate(v)) => {
                Some(vote_instruction::update_vote_state(
                    vote_account_pubkey,
                    authorized_voter_pubkey,
                    v,
                ))
            }
            (SwitchForkDecision::SwitchProof(switch_proof_hash), VoteTransaction::Vote(v)) => {
                Some(vote_instruction::vote_switch(
                    vote_account_pubkey,
                    authorized_voter_pubkey,
                    v,
                    *switch_proof_hash,
                ))
            }
            (
                SwitchForkDecision::SwitchProof(switch_proof_hash),
                VoteTransaction::VoteStateUpdate(v),
            ) => Some(vote_instruction::update_vote_state_switch(
                vote_account_pubkey,
                authorized_voter_pubkey,
                v,
                *switch_proof_hash,
            )),
            (SwitchForkDecision::SameFork, VoteTransaction::CompactVoteStateUpdate(v)) => {
                Some(vote_instruction::compact_update_vote_state(
                    vote_account_pubkey,
                    authorized_voter_pubkey,
                    v,
                ))
            }
            (
                SwitchForkDecision::SwitchProof(switch_proof_hash),
                VoteTransaction::CompactVoteStateUpdate(v),
            ) => Some(vote_instruction::compact_update_vote_state_switch(
                vote_account_pubkey,
                authorized_voter_pubkey,
                v,
                *switch_proof_hash,
            )),
        }
    }

    pub fn can_vote(&self) -> bool {
        match self {
            SwitchForkDecision::FailedSwitchThreshold(_, _) => false,
            SwitchForkDecision::FailedSwitchDuplicateRollback(_) => false,
            SwitchForkDecision::SameFork => true,
            SwitchForkDecision::SwitchProof(_) => true,
        }
    }
}

pub const VOTE_THRESHOLD_DEPTH: usize = 8;
pub const SWITCH_FORK_THRESHOLD: f64 = 0.38;

pub type Result<T> = std::result::Result<T, TowerError>;

pub type Stake = u64;
pub type VotedStakes = HashMap<Slot, Stake>;
pub type PubkeyVotes = Vec<(Pubkey, Slot)>;

pub(crate) struct ComputedBankState {
    pub voted_stakes: VotedStakes,
    pub total_stake: Stake,
    #[allow(dead_code)]
    bank_weight: u128,
    // Tree of intervals of lockouts of the form [slot, slot + slot.lockout],
    // keyed by end of the range
    pub lockout_intervals: LockoutIntervals,
    pub my_latest_landed_vote: Option<Slot>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum TowerVersions {
    V1_17_14(Tower1_7_14),
    V1_14_11(Tower1_14_11),
    Current(Tower),
}

impl TowerVersions {
    pub fn new_current(tower: Tower) -> Self {
        Self::Current(tower)
    }

    pub fn convert_to_current(self) -> Tower {
        match self {
            TowerVersions::V1_17_14(tower) => {
                let box_last_vote = VoteTransaction::from(tower.last_vote.clone());

                Tower {
                    node_pubkey: tower.node_pubkey,
                    threshold_depth: tower.threshold_depth,
                    threshold_size: tower.threshold_size,
                    vote_state: VoteStateVersions::V1_14_11(Box::new(tower.vote_state))
                        .convert_to_current(),
                    last_vote: box_last_vote,
                    last_vote_tx_blockhash: tower.last_vote_tx_blockhash,
                    last_timestamp: tower.last_timestamp,
                    stray_restored_slot: tower.stray_restored_slot,
                    last_switch_threshold_check: tower.last_switch_threshold_check,
                }
            }
            TowerVersions::V1_14_11(tower) => Tower {
                node_pubkey: tower.node_pubkey,
                threshold_depth: tower.threshold_depth,
                threshold_size: tower.threshold_size,
                vote_state: VoteStateVersions::V1_14_11(Box::new(tower.vote_state))
                    .convert_to_current(),
                last_vote: tower.last_vote,
                last_vote_tx_blockhash: tower.last_vote_tx_blockhash,
                last_timestamp: tower.last_timestamp,
                stray_restored_slot: tower.stray_restored_slot,
                last_switch_threshold_check: tower.last_switch_threshold_check,
            },
            TowerVersions::Current(tower) => tower,
        }
    }
}

#[frozen_abi(digest = "iZi6s9BvytU3HbRsibrAD71jwMLvrqHdCjVk6qKcVvd")]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, AbiExample)]
pub struct Tower {
    pub node_pubkey: Pubkey,
    threshold_depth: usize,
    threshold_size: f64,
    pub(crate) vote_state: VoteState,
    last_vote: VoteTransaction,
    #[serde(skip)]
    // The blockhash used in the last vote transaction, may or may not equal the
    // blockhash of the voted block itself, depending if the vote slot was refreshed.
    // For instance, a vote for slot 5, may be refreshed/resubmitted for inclusion in
    //  block 10, in  which case `last_vote_tx_blockhash` equals the blockhash of 10, not 5.
    // For non voting validators this is None
    last_vote_tx_blockhash: Option<Hash>,
    last_timestamp: BlockTimestamp,
    #[serde(skip)]
    // Restored last voted slot which cannot be found in SlotHistory at replayed root
    // (This is a special field for slashing-free validator restart with edge cases).
    // This could be emptied after some time; but left intact indefinitely for easier
    // implementation
    // Further, stray slot can be stale or not. `Stale` here means whether given
    // bank_forks (=~ ledger) lacks the slot or not.
    stray_restored_slot: Option<Slot>,
    #[serde(skip)]
    pub last_switch_threshold_check: Option<(Slot, SwitchForkDecision)>,
}

impl Default for Tower {
    fn default() -> Self {
        let mut tower = Self {
            node_pubkey: Pubkey::default(),
            threshold_depth: VOTE_THRESHOLD_DEPTH,
            threshold_size: VOTE_THRESHOLD_SIZE,
            vote_state: VoteState::default(),
            last_vote: VoteTransaction::from(VoteStateUpdate::default()),
            last_timestamp: BlockTimestamp::default(),
            last_vote_tx_blockhash: None,
            stray_restored_slot: Option::default(),
            last_switch_threshold_check: Option::default(),
        };
        // VoteState::root_slot is ensured to be Some in Tower
        tower.vote_state.root_slot = Some(Slot::default());
        tower
    }
}

impl Tower {
    pub fn new(
        node_pubkey: &Pubkey,
        vote_account_pubkey: &Pubkey,
        root: Slot,
        bank: &Bank,
    ) -> Self {
        let mut tower = Tower {
            node_pubkey: *node_pubkey,
            ..Tower::default()
        };
        tower.initialize_lockouts_from_bank(vote_account_pubkey, root, bank);
        tower
    }

    #[cfg(test)]
    pub fn new_for_tests(threshold_depth: usize, threshold_size: f64) -> Self {
        Self {
            threshold_depth,
            threshold_size,
            ..Tower::default()
        }
    }

    pub fn new_from_bankforks(
        bank_forks: &BankForks,
        node_pubkey: &Pubkey,
        vote_account: &Pubkey,
    ) -> Self {
        let root_bank = bank_forks.root_bank();
        let (_progress, heaviest_subtree_fork_choice) =
            crate::replay_stage::ReplayStage::initialize_progress_and_fork_choice(
                root_bank.deref(),
                bank_forks.frozen_banks().values().cloned().collect(),
                node_pubkey,
                vote_account,
                vec![],
            );
        let root = root_bank.slot();

        let (best_slot, best_hash) = heaviest_subtree_fork_choice.best_overall_slot();
        let heaviest_bank = bank_forks
            .get_with_checked_hash((best_slot, best_hash))
            .expect(
                "The best overall slot must be one of `frozen_banks` which all exist in bank_forks",
            );

        Self::new(node_pubkey, vote_account, root, &heaviest_bank)
    }

    pub(crate) fn collect_vote_lockouts(
        vote_account_pubkey: &Pubkey,
        bank_slot: Slot,
        vote_accounts: &VoteAccountsHashMap,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
        get_frozen_hash: impl Fn(Slot) -> Option<Hash>,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
    ) -> ComputedBankState {
        let mut vote_slots = HashSet::new();
        let mut voted_stakes = HashMap::new();
        let mut total_stake = 0;
        let mut bank_weight = 0;
        // Tree of intervals of lockouts of the form [slot, slot + slot.lockout],
        // keyed by end of the range
        let mut lockout_intervals = LockoutIntervals::new();
        let mut my_latest_landed_vote = None;
        for (&key, (voted_stake, account)) in vote_accounts.iter() {
            let voted_stake = *voted_stake;
            if voted_stake == 0 {
                continue;
            }
            trace!("{} {} with stake {}", vote_account_pubkey, key, voted_stake);
            let mut vote_state = match account.vote_state().cloned() {
                Err(_) => {
                    datapoint_warn!(
                        "tower_warn",
                        (
                            "warn",
                            format!("Unable to get vote_state from account {key}"),
                            String
                        ),
                    );
                    continue;
                }
                Ok(vote_state) => vote_state,
            };
            for vote in &vote_state.votes {
                lockout_intervals
                    .entry(vote.lockout.last_locked_out_slot())
                    .or_default()
                    .push((vote.slot(), key));
            }

            if key == *vote_account_pubkey {
                my_latest_landed_vote = vote_state.nth_recent_lockout(0).map(|l| l.slot());
                debug!("vote state {:?}", vote_state);
                debug!(
                    "observed slot {}",
                    vote_state
                        .nth_recent_lockout(0)
                        .map(|l| l.slot())
                        .unwrap_or(0) as i64
                );
                debug!("observed root {}", vote_state.root_slot.unwrap_or(0) as i64);
                datapoint_info!(
                    "tower-observed",
                    (
                        "slot",
                        vote_state
                            .nth_recent_lockout(0)
                            .map(|l| l.slot())
                            .unwrap_or(0),
                        i64
                    ),
                    ("root", vote_state.root_slot.unwrap_or(0), i64)
                );
            }
            let start_root = vote_state.root_slot;

            // Add the last vote to update the `heaviest_subtree_fork_choice`
            if let Some(last_landed_voted_slot) = vote_state.last_voted_slot() {
                latest_validator_votes_for_frozen_banks.check_add_vote(
                    key,
                    last_landed_voted_slot,
                    get_frozen_hash(last_landed_voted_slot),
                    true,
                );
            }

            process_slot_vote_unchecked(&mut vote_state, bank_slot);

            for vote in &vote_state.votes {
                bank_weight += vote.lockout.lockout() as u128 * voted_stake as u128;
                vote_slots.insert(vote.slot());
            }

            if start_root != vote_state.root_slot {
                if let Some(root) = start_root {
                    let vote =
                        Lockout::new_with_confirmation_count(root, MAX_LOCKOUT_HISTORY as u32);
                    trace!("ROOT: {}", vote.slot());
                    bank_weight += vote.lockout() as u128 * voted_stake as u128;
                    vote_slots.insert(vote.slot());
                }
            }
            if let Some(root) = vote_state.root_slot {
                let vote = Lockout::new_with_confirmation_count(root, MAX_LOCKOUT_HISTORY as u32);
                bank_weight += vote.lockout() as u128 * voted_stake as u128;
                vote_slots.insert(vote.slot());
            }

            // The last vote in the vote stack is a simulated vote on bank_slot, which
            // we added to the vote stack earlier in this function by calling process_vote().
            // We don't want to update the ancestors stakes of this vote b/c it does not
            // represent an actual vote by the validator.

            // Note: It should not be possible for any vote state in this bank to have
            // a vote for a slot >= bank_slot, so we are guaranteed that the last vote in
            // this vote stack is the simulated vote, so this fetch should be sufficient
            // to find the last unsimulated vote.
            assert_eq!(
                vote_state.nth_recent_lockout(0).map(|l| l.slot()),
                Some(bank_slot)
            );
            if let Some(vote) = vote_state.nth_recent_lockout(1) {
                // Update all the parents of this last vote with the stake of this vote account
                Self::update_ancestor_voted_stakes(
                    &mut voted_stakes,
                    vote.slot(),
                    voted_stake,
                    ancestors,
                );
            }
            total_stake += voted_stake;
        }

        // TODO: populate_ancestor_voted_stakes only adds zeros. Comment why
        // that is necessary (if so).
        Self::populate_ancestor_voted_stakes(&mut voted_stakes, vote_slots, ancestors);
        ComputedBankState {
            voted_stakes,
            total_stake,
            bank_weight,
            lockout_intervals,
            my_latest_landed_vote,
        }
    }

    pub fn is_slot_confirmed(
        &self,
        slot: Slot,
        voted_stakes: &VotedStakes,
        total_stake: Stake,
    ) -> bool {
        voted_stakes
            .get(&slot)
            .map(|stake| (*stake as f64 / total_stake as f64) > self.threshold_size)
            .unwrap_or(false)
    }

    pub fn tower_slots(&self) -> Vec<Slot> {
        self.vote_state.tower()
    }

    pub fn last_vote_tx_blockhash(&self) -> Option<Hash> {
        self.last_vote_tx_blockhash
    }

    pub fn refresh_last_vote_timestamp(&mut self, heaviest_slot_on_same_fork: Slot) {
        let timestamp = if let Some(last_vote_timestamp) = self.last_vote.timestamp() {
            // To avoid a refreshed vote tx getting caught in deduplication filters,
            // we need to update timestamp. Increment by smallest amount to avoid skewing
            // the Timestamp Oracle.
            last_vote_timestamp.saturating_add(1)
        } else {
            // If the previous vote did not send a timestamp due to clock error,
            // use the last good timestamp + 1
            datapoint_info!(
                "refresh-timestamp-missing",
                ("heaviest-slot", heaviest_slot_on_same_fork, i64),
                ("last-timestamp", self.last_timestamp.timestamp, i64),
                ("last-slot", self.last_timestamp.slot, i64),
            );
            self.last_timestamp.timestamp.saturating_add(1)
        };

        if let Some(last_voted_slot) = self.last_vote.last_voted_slot() {
            if heaviest_slot_on_same_fork <= last_voted_slot {
                warn!(
                    "Trying to refresh timestamp for vote on {last_voted_slot}
                     using smaller heaviest bank {heaviest_slot_on_same_fork}"
                );
                return;
            }
            self.last_timestamp = BlockTimestamp {
                slot: last_voted_slot,
                timestamp,
            };
            self.last_vote.set_timestamp(Some(timestamp));
        } else {
            warn!(
                "Trying to refresh timestamp for last vote on heaviest bank on same fork
                   {heaviest_slot_on_same_fork}, but there is no vote to refresh"
            );
        }
    }

    pub fn refresh_last_vote_tx_blockhash(&mut self, new_vote_tx_blockhash: Hash) {
        self.last_vote_tx_blockhash = Some(new_vote_tx_blockhash);
    }

    pub fn last_voted_slot_in_bank(bank: &Bank, vote_account_pubkey: &Pubkey) -> Option<Slot> {
        let vote_account = bank.get_vote_account(vote_account_pubkey)?;
        let vote_state = vote_account.vote_state();
        vote_state.as_ref().ok()?.last_voted_slot()
    }

    pub fn record_bank_vote(&mut self, bank: &Bank) -> Option<Slot> {
        // Returns the new root if one is made after applying a vote for the given bank to
        // `self.vote_state`
        self.record_bank_vote_and_update_lockouts(bank.slot(), bank.hash())
    }

    /// If we've recently updated the vote state by applying a new vote
    /// or syncing from a bank, generate the proper last_vote.
    pub(crate) fn update_last_vote_from_vote_state(&mut self, vote_hash: Hash) {
        let mut new_vote = VoteTransaction::from(VoteStateUpdate::new(
            self.vote_state
                .votes
                .iter()
                .map(|vote| vote.lockout)
                .collect(),
            self.vote_state.root_slot,
            vote_hash,
        ));

        new_vote.set_timestamp(self.maybe_timestamp(self.last_voted_slot().unwrap_or_default()));
        self.last_vote = new_vote;
    }

    fn record_bank_vote_and_update_lockouts(
        &mut self,
        vote_slot: Slot,
        vote_hash: Hash,
    ) -> Option<Slot> {
        trace!("{} record_vote for {}", self.node_pubkey, vote_slot);
        let old_root = self.root();

        let vote = Vote::new(vec![vote_slot], vote_hash);
        let result = process_vote_unchecked(&mut self.vote_state, vote);
        if result.is_err() {
            panic!(
                "Error while recording vote {} {} in local tower {:?}",
                vote_slot, vote_hash, result
            );
        }
        self.update_last_vote_from_vote_state(vote_hash);

        let new_root = self.root();

        datapoint_info!(
            "tower-vote",
            ("latest", vote_slot, i64),
            ("root", new_root, i64)
        );
        if old_root != new_root {
            Some(new_root)
        } else {
            None
        }
    }

    #[cfg(test)]
    pub fn record_vote(&mut self, slot: Slot, hash: Hash) -> Option<Slot> {
        self.record_bank_vote_and_update_lockouts(slot, hash)
    }

    /// Used for tests
    pub fn increase_lockout(&mut self, confirmation_count_increase: u32) {
        for vote in self.vote_state.votes.iter_mut() {
            vote.lockout
                .increase_confirmation_count(confirmation_count_increase);
        }
    }

    pub fn last_voted_slot(&self) -> Option<Slot> {
        if self.last_vote.is_empty() {
            None
        } else {
            Some(self.last_vote.slot(self.last_vote.len() - 1))
        }
    }

    pub fn last_voted_slot_hash(&self) -> Option<(Slot, Hash)> {
        Some((self.last_voted_slot()?, self.last_vote.hash()))
    }

    pub fn stray_restored_slot(&self) -> Option<Slot> {
        self.stray_restored_slot
    }

    pub fn last_vote(&self) -> VoteTransaction {
        self.last_vote.clone()
    }

    fn maybe_timestamp(&mut self, current_slot: Slot) -> Option<UnixTimestamp> {
        if current_slot > self.last_timestamp.slot
            || self.last_timestamp.slot == 0 && current_slot == self.last_timestamp.slot
        {
            let timestamp = Utc::now().timestamp();
            if timestamp >= self.last_timestamp.timestamp {
                self.last_timestamp = BlockTimestamp {
                    slot: current_slot,
                    timestamp,
                };
                return Some(timestamp);
            } else {
                datapoint_info!(
                    "backwards-timestamp",
                    ("slot", current_slot, i64),
                    ("timestamp", timestamp, i64),
                    ("last-timestamp", self.last_timestamp.timestamp, i64),
                )
            }
        }
        None
    }

    // root may be forcibly set by arbitrary replay root slot, for example from a root
    // after replaying a snapshot.
    // Also, tower.root() couldn't be None; initialize_lockouts() ensures that.
    // Conceptually, every tower must have been constructed from a concrete starting point,
    // which establishes the origin of trust (i.e. root) whether booting from genesis (slot 0) or
    // snapshot (slot N). In other words, there should be no possibility a Tower doesn't have
    // root, unlike young vote accounts.
    pub fn root(&self) -> Slot {
        self.vote_state.root_slot.unwrap()
    }

    // a slot is recent if it's newer than the last vote we have. If we haven't voted yet
    // but have a root (hard forks situation) then compare it to the root
    pub fn is_recent(&self, slot: Slot) -> bool {
        if let Some(last_voted_slot) = self.vote_state.last_voted_slot() {
            if slot <= last_voted_slot {
                return false;
            }
        } else if let Some(root) = self.vote_state.root_slot {
            if slot <= root {
                return false;
            }
        }
        true
    }

    pub fn has_voted(&self, slot: Slot) -> bool {
        for vote in &self.vote_state.votes {
            if slot == vote.slot() {
                return true;
            }
        }
        false
    }

    pub fn is_locked_out(&self, slot: Slot, ancestors: &HashSet<Slot>) -> bool {
        if !self.is_recent(slot) {
            return true;
        }

        // Check if a slot is locked out by simulating adding a vote for that
        // slot to the current lockouts to pop any expired votes. If any of the
        // remaining voted slots are on a different fork from the checked slot,
        // it's still locked out.
        let mut vote_state = self.vote_state.clone();
        process_slot_vote_unchecked(&mut vote_state, slot);
        for vote in &vote_state.votes {
            if slot != vote.slot() && !ancestors.contains(&vote.slot()) {
                return true;
            }
        }

        if let Some(root_slot) = vote_state.root_slot {
            if slot != root_slot {
                // This case should never happen because bank forks purges all
                // non-descendants of the root every time root is set
                assert!(
                    ancestors.contains(&root_slot),
                    "ancestors: {ancestors:?}, slot: {slot} root: {root_slot}"
                );
            }
        }

        false
    }

    fn is_candidate_slot_descendant_of_last_vote(
        candidate_slot: Slot,
        last_voted_slot: Slot,
        ancestors: &HashMap<Slot, HashSet<u64>>,
    ) -> Option<bool> {
        ancestors
            .get(&candidate_slot)
            .map(|candidate_slot_ancestors| candidate_slot_ancestors.contains(&last_voted_slot))
    }

    #[allow(clippy::too_many_arguments)]
    fn make_check_switch_threshold_decision(
        &self,
        switch_slot: Slot,
        ancestors: &HashMap<Slot, HashSet<u64>>,
        descendants: &HashMap<Slot, HashSet<u64>>,
        progress: &ProgressMap,
        total_stake: u64,
        epoch_vote_accounts: &VoteAccountsHashMap,
        latest_validator_votes_for_frozen_banks: &LatestValidatorVotesForFrozenBanks,
        heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice,
    ) -> SwitchForkDecision {
        let Some((last_voted_slot, last_voted_hash)) = self.last_voted_slot_hash() else {
            return SwitchForkDecision::SameFork;
        };
        let root = self.root();
        let empty_ancestors = HashSet::default();
        let empty_ancestors_due_to_minor_unsynced_ledger = || {
            // This condition (stale stray last vote) shouldn't occur under normal validator
            // operation, indicating something unusual happened.
            // This condition could be introduced by manual ledger mishandling,
            // validator SEGV, OS/HW crash, or plain No Free Space FS error.

            // However, returning empty ancestors as a fallback here shouldn't result in
            // slashing by itself (Note that we couldn't fully preclude any kind of slashing if
            // the failure was OS or HW level).

            // Firstly, lockout is ensured elsewhere.

            // Also, there is no risk of optimistic conf. violation. Although empty ancestors
            // could result in incorrect (= more than actual) locked_out_stake and
            // false-positive SwitchProof later in this function, there should be no such a
            // heavier fork candidate, first of all, if the last vote (or any of its
            // unavailable ancestors) were already optimistically confirmed.
            // The only exception is that other validator is already violating it...
            if self.is_first_switch_check() && switch_slot < last_voted_slot {
                // `switch < last` is needed not to warn! this message just because of using
                // newer snapshots on validator restart
                let message = format!(
                            "bank_forks doesn't have corresponding data for the stray restored \
                           last vote({last_voted_slot}), meaning some inconsistency between saved tower and ledger."
                        );
                warn!("{}", message);
                datapoint_warn!("tower_warn", ("warn", message, String));
            }
            &empty_ancestors
        };

        let suspended_decision_due_to_major_unsynced_ledger = || {
            // This peculiar corner handling is needed mainly for a tower which is newer than
            // blockstore. (Yeah, we tolerate it for ease of maintaining validator by operators)
            // This condition could be introduced by manual ledger mishandling,
            // validator SEGV, OS/HW crash, or plain No Free Space FS error.

            // When we're in this clause, it basically means validator is badly running
            // with a future tower while replaying past slots, especially problematic is
            // last_voted_slot.
            // So, don't re-vote on it by returning pseudo FailedSwitchThreshold, otherwise
            // there would be slashing because of double vote on one of last_vote_ancestors.
            // (Well, needless to say, re-creating the duplicate block must be handled properly
            // at the banking stage: https://github.com/solana-labs/solana/issues/8232)
            //
            // To be specific, the replay stage is tricked into a false perception where
            // last_vote_ancestors is AVAILABLE for descendant-of-`switch_slot`,  stale, and
            // stray slots (which should always be empty_ancestors).
            //
            // This is covered by test_future_tower_* in local_cluster
            SwitchForkDecision::FailedSwitchThreshold(0, total_stake)
        };

        let rollback_due_to_to_to_duplicate_ancestor = |latest_duplicate_ancestor| {
            SwitchForkDecision::FailedSwitchDuplicateRollback(latest_duplicate_ancestor)
        };

        // `heaviest_subtree_fork_choice` entries are not cleaned by duplicate block purging/rollback logic,
        // so this is safe to check here. We return here if the last voted slot was rolled back/purged due to
        // being a duplicate because `ancestors`/`descendants`/`progress` structures may be missing this slot due
        // to duplicate purging. This would cause many of the `unwrap()` checks below to fail.
        //
        // TODO: Handle if the last vote is on a dupe, and then we restart. The dupe won't be in
        // heaviest_subtree_fork_choice, so `heaviest_subtree_fork_choice.latest_invalid_ancestor()` will return
        // None, but the last vote will be persisted in tower.
        let switch_hash = progress
            .get_hash(switch_slot)
            .expect("Slot we're trying to switch to must exist AND be frozen in progress map");
        if let Some(latest_duplicate_ancestor) = heaviest_subtree_fork_choice
            .latest_invalid_ancestor(&(last_voted_slot, last_voted_hash))
        {
            // We're rolling back because one of the ancestors of the last vote was a duplicate. In this
            // case, it's acceptable if the switch candidate is one of ancestors of the previous vote,
            // just fail the switch check because there's no point in voting on an ancestor. ReplayStage
            // should then have a special case continue building an alternate fork from this ancestor, NOT
            // the `last_voted_slot`. This is in contrast to usual SwitchFailure where ReplayStage continues to build blocks
            // on latest vote. See `ReplayStage::select_vote_and_reset_forks()` for more details.
            if heaviest_subtree_fork_choice.is_strict_ancestor(
                &(switch_slot, switch_hash),
                &(last_voted_slot, last_voted_hash),
            ) {
                return rollback_due_to_to_to_duplicate_ancestor(latest_duplicate_ancestor);
            } else if progress
                .get_hash(last_voted_slot)
                .map(|current_slot_hash| current_slot_hash != last_voted_hash)
                .unwrap_or(true)
            {
                // Our last vote slot was purged because it was on a duplicate fork, don't continue below
                // where checks may panic. We allow a freebie vote here that may violate switching
                // thresholds
                // TODO: Properly handle this case
                info!(
                    "Allowing switch vote on {:?} because last vote {:?} was rolled back",
                    (switch_slot, switch_hash),
                    (last_voted_slot, last_voted_hash)
                );
                return SwitchForkDecision::SwitchProof(Hash::default());
            }
        }

        let last_vote_ancestors = ancestors.get(&last_voted_slot).unwrap_or_else(|| {
            if self.is_stray_last_vote() {
                // Unless last vote is stray and stale, ancestors.get(last_voted_slot) must
                // return Some(_), justifying to panic! here.
                // Also, adjust_lockouts_after_replay() correctly makes last_voted_slot None,
                // if all saved votes are ancestors of replayed_root_slot. So this code shouldn't be
                // touched in that case as well.
                // In other words, except being stray, all other slots have been voted on while
                // this validator has been running, so we must be able to fetch ancestors for
                // all of them.
                empty_ancestors_due_to_minor_unsynced_ledger()
            } else {
                panic!("no ancestors found with slot: {last_voted_slot}");
            }
        });

        let switch_slot_ancestors = ancestors.get(&switch_slot).unwrap();

        if switch_slot == last_voted_slot || switch_slot_ancestors.contains(&last_voted_slot) {
            // If the `switch_slot is a descendant of the last vote,
            // no switching proof is necessary
            return SwitchForkDecision::SameFork;
        }

        if last_vote_ancestors.contains(&switch_slot) {
            if self.is_stray_last_vote() {
                return suspended_decision_due_to_major_unsynced_ledger();
            } else {
                panic!(
                            "Should never consider switching to ancestor ({switch_slot}) of last vote: {last_voted_slot}, ancestors({last_vote_ancestors:?})",
                        );
            }
        }

        // By this point, we know the `switch_slot` is on a different fork
        // (is neither an ancestor nor descendant of `last_vote`), so a
        // switching proof is necessary
        let switch_proof = Hash::default();
        let mut locked_out_stake = 0;
        let mut locked_out_vote_accounts = HashSet::new();
        for (candidate_slot, descendants) in descendants.iter() {
            // 1) Don't consider any banks that haven't been frozen yet
            //    because the needed stats are unavailable
            // 2) Only consider lockouts at the latest `frozen` bank
            //    on each fork, as that bank will contain all the
            //    lockout intervals for ancestors on that fork as well.
            // 3) Don't consider lockouts on the `last_vote` itself
            // 4) Don't consider lockouts on any descendants of
            //    `last_vote`
            // 5) Don't consider any banks before the root because
            //    all lockouts must be ancestors of `last_vote`
            if !progress
                .get_fork_stats(*candidate_slot)
                .map(|stats| stats.computed)
                .unwrap_or(false)
                || {
                    // If any of the descendants have the `computed` flag set, then there must be a more
                    // recent frozen bank on this fork to use, so we can ignore this one. Otherwise,
                    // even if this bank has descendants, if they have not yet been frozen / stats computed,
                    // then use this bank as a representative for the fork.
                    descendants.iter().any(|d| {
                        progress
                            .get_fork_stats(*d)
                            .map(|stats| stats.computed)
                            .unwrap_or(false)
                    })
                }
                || *candidate_slot == last_voted_slot
                || {
                    // Ignore if the `candidate_slot` is a descendant of the `last_voted_slot`, since we do not
                    // want to count votes on the same fork.
                    Self::is_candidate_slot_descendant_of_last_vote(
                        *candidate_slot,
                        last_voted_slot,
                        ancestors,
                    )
                    .expect("exists in descendants map, so must exist in ancestors map")
                }
                || *candidate_slot <= root
            {
                continue;
            }

            // By the time we reach here, any ancestors of the `last_vote`,
            // should have been filtered out, as they all have a descendant,
            // namely the `last_vote` itself.
            assert!(!last_vote_ancestors.contains(candidate_slot));

            // Evaluate which vote accounts in the bank are locked out
            // in the interval candidate_slot..last_vote, which means
            // finding any lockout intervals in the `lockout_intervals` tree
            // for this bank that contain `last_vote`.
            let lockout_intervals = &progress
                .get(candidate_slot)
                .unwrap()
                .fork_stats
                .lockout_intervals;
            // Find any locked out intervals for vote accounts in this bank with
            // `lockout_interval_end` >= `last_vote`, which implies they are locked out at
            // `last_vote` on another fork.
            for (_lockout_interval_end, intervals_keyed_by_end) in
                lockout_intervals.range((Included(last_voted_slot), Unbounded))
            {
                for (lockout_interval_start, vote_account_pubkey) in intervals_keyed_by_end {
                    if locked_out_vote_accounts.contains(vote_account_pubkey) {
                        continue;
                    }

                    // Only count lockouts on slots that are:
                    // 1) Not ancestors of `last_vote`, meaning being on different fork
                    // 2) Not from before the current root as we can't determine if
                    // anything before the root was an ancestor of `last_vote` or not
                    if !last_vote_ancestors.contains(lockout_interval_start) && {
                        // Given a `lockout_interval_start` < root that appears in a
                        // bank for a `candidate_slot`, it must be that `lockout_interval_start`
                        // is an ancestor of the current root, because `candidate_slot` is a
                        // descendant of the current root
                        *lockout_interval_start > root
                    } {
                        let stake = epoch_vote_accounts
                            .get(vote_account_pubkey)
                            .map(|(stake, _)| *stake)
                            .unwrap_or(0);
                        locked_out_stake += stake;
                        if (locked_out_stake as f64 / total_stake as f64) > SWITCH_FORK_THRESHOLD {
                            return SwitchForkDecision::SwitchProof(switch_proof);
                        }
                        locked_out_vote_accounts.insert(vote_account_pubkey);
                    }
                }
            }
        }

        // Check the latest votes for potentially gossip votes that haven't landed yet
        for (
            vote_account_pubkey,
            (candidate_latest_frozen_vote, _candidate_latest_frozen_vote_hash),
        ) in latest_validator_votes_for_frozen_banks.max_gossip_frozen_votes()
        {
            if locked_out_vote_accounts.contains(&vote_account_pubkey) {
                continue;
            }

            if *candidate_latest_frozen_vote > last_voted_slot && {
                // Because `candidate_latest_frozen_vote` is the last vote made by some validator
                // in the cluster for a frozen bank `B` observed through gossip, we may have cleared
                // that frozen bank `B` because we `set_root(root)` for a `root` on a different fork,
                // like so:
                //
                //    |----------X ------candidate_latest_frozen_vote (frozen)
                // old root
                //    |----------new root ----last_voted_slot
                //
                // In most cases, because `last_voted_slot` must be a descendant of `root`, then
                // if `candidate_latest_frozen_vote` is not found in the ancestors/descendants map (recall these
                // directly reflect the state of BankForks), this implies that `B` was pruned from BankForks
                // because it was on a different fork than `last_voted_slot`, and thus this vote for `candidate_latest_frozen_vote`
                // should be safe to count towards the switching proof:
                //
                // However, there is also the possibility that `last_voted_slot` is a stray, in which
                // case we cannot make this conclusion as we do not know the ancestors/descendants
                // of strays. Hence we err on the side of caution here and ignore this vote. This
                // is ok because validators voting on different unrooted forks should eventually vote
                // on some descendant of the root, at which time they can be included in switching proofs.
                !Self::is_candidate_slot_descendant_of_last_vote(
                    *candidate_latest_frozen_vote,
                    last_voted_slot,
                    ancestors,
                )
                .unwrap_or(true)
            } {
                let stake = epoch_vote_accounts
                    .get(vote_account_pubkey)
                    .map(|(stake, _)| *stake)
                    .unwrap_or(0);
                locked_out_stake += stake;
                if (locked_out_stake as f64 / total_stake as f64) > SWITCH_FORK_THRESHOLD {
                    return SwitchForkDecision::SwitchProof(switch_proof);
                }
                locked_out_vote_accounts.insert(vote_account_pubkey);
            }
        }

        // We have not detected sufficient lockout past the last voted slot to generate
        // a switching proof
        SwitchForkDecision::FailedSwitchThreshold(locked_out_stake, total_stake)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_switch_threshold(
        &mut self,
        switch_slot: Slot,
        ancestors: &HashMap<Slot, HashSet<u64>>,
        descendants: &HashMap<Slot, HashSet<u64>>,
        progress: &ProgressMap,
        total_stake: u64,
        epoch_vote_accounts: &VoteAccountsHashMap,
        latest_validator_votes_for_frozen_banks: &LatestValidatorVotesForFrozenBanks,
        heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice,
    ) -> SwitchForkDecision {
        let decision = self.make_check_switch_threshold_decision(
            switch_slot,
            ancestors,
            descendants,
            progress,
            total_stake,
            epoch_vote_accounts,
            latest_validator_votes_for_frozen_banks,
            heaviest_subtree_fork_choice,
        );
        let new_check = Some((switch_slot, decision.clone()));
        if new_check != self.last_switch_threshold_check {
            trace!(
                "new switch threshold check: slot {}: {:?}",
                switch_slot,
                decision,
            );
            self.last_switch_threshold_check = new_check;
        }
        decision
    }

    fn is_first_switch_check(&self) -> bool {
        self.last_switch_threshold_check.is_none()
    }

    /// Performs threshold check for `slot`
    ///
    /// If it passes the check returns None, otherwise returns Some(fork_stake)
    pub fn check_vote_stake_threshold(
        &self,
        slot: Slot,
        voted_stakes: &VotedStakes,
        total_stake: Stake,
    ) -> ThresholdDecision {
        let mut vote_state = self.vote_state.clone();
        process_slot_vote_unchecked(&mut vote_state, slot);
        let lockout = vote_state.nth_recent_lockout(self.threshold_depth);
        if let Some(lockout) = lockout {
            if let Some(fork_stake) = voted_stakes.get(&lockout.slot()) {
                let lockout_stake = *fork_stake as f64 / total_stake as f64;
                trace!(
                    "fork_stake slot: {}, vote slot: {}, lockout: {} fork_stake: {} total_stake: {}",
                    slot, lockout.slot(), lockout_stake, fork_stake, total_stake
                );
                if lockout.confirmation_count() as usize > self.threshold_depth {
                    for old_vote in &self.vote_state.votes {
                        if old_vote.slot() == lockout.slot()
                            && old_vote.confirmation_count() == lockout.confirmation_count()
                        {
                            return ThresholdDecision::PassedThreshold;
                        }
                    }
                }

                if lockout_stake > self.threshold_size {
                    return ThresholdDecision::PassedThreshold;
                }
                ThresholdDecision::FailedThreshold(*fork_stake)
            } else {
                // We haven't seen any votes on this fork yet, so no stake
                ThresholdDecision::FailedThreshold(0)
            }
        } else {
            ThresholdDecision::PassedThreshold
        }
    }

    /// Update lockouts for all the ancestors
    pub(crate) fn populate_ancestor_voted_stakes(
        voted_stakes: &mut VotedStakes,
        vote_slots: impl IntoIterator<Item = Slot>,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
    ) {
        // If there's no ancestors, that means this slot must be from before the current root,
        // in which case the lockouts won't be calculated in bank_weight anyways, so ignore
        // this slot
        for vote_slot in vote_slots {
            if let Some(slot_ancestors) = ancestors.get(&vote_slot) {
                voted_stakes.entry(vote_slot).or_default();
                for slot in slot_ancestors {
                    voted_stakes.entry(*slot).or_default();
                }
            }
        }
    }

    /// Update stake for all the ancestors.
    /// Note, stake is the same for all the ancestor.
    fn update_ancestor_voted_stakes(
        voted_stakes: &mut VotedStakes,
        voted_slot: Slot,
        voted_stake: u64,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
    ) {
        // If there's no ancestors, that means this slot must be from
        // before the current root, so ignore this slot
        if let Some(vote_slot_ancestors) = ancestors.get(&voted_slot) {
            *voted_stakes.entry(voted_slot).or_default() += voted_stake;
            for slot in vote_slot_ancestors {
                *voted_stakes.entry(*slot).or_default() += voted_stake;
            }
        }
    }

    fn voted_slots(&self) -> Vec<Slot> {
        self.vote_state
            .votes
            .iter()
            .map(|lockout| lockout.slot())
            .collect()
    }

    pub fn is_stray_last_vote(&self) -> bool {
        self.stray_restored_slot.is_some() && self.stray_restored_slot == self.last_voted_slot()
    }

    // The tower root can be older/newer if the validator booted from a newer/older snapshot, so
    // tower lockouts may need adjustment
    pub fn adjust_lockouts_after_replay(
        mut self,
        replayed_root: Slot,
        slot_history: &SlotHistory,
    ) -> Result<Self> {
        // sanity assertions for roots
        let tower_root = self.root();
        info!(
            "adjusting lockouts (after replay up to {}): {:?} tower root: {} replayed root: {}",
            replayed_root,
            self.voted_slots(),
            tower_root,
            replayed_root,
        );
        assert_eq!(slot_history.check(replayed_root), Check::Found);

        assert!(
            self.last_vote == VoteTransaction::from(VoteStateUpdate::default())
                && self.vote_state.votes.is_empty()
                || self.last_vote != VoteTransaction::from(VoteStateUpdate::default())
                    && !self.vote_state.votes.is_empty(),
            "last vote: {:?} vote_state.votes: {:?}",
            self.last_vote,
            self.vote_state.votes
        );

        if let Some(last_voted_slot) = self.last_voted_slot() {
            if tower_root <= replayed_root {
                // Normally, we goes into this clause with possible help of
                // reconcile_blockstore_roots_with_external_source()
                if slot_history.check(last_voted_slot) == Check::TooOld {
                    // We could try hard to anchor with other older votes, but opt to simplify the
                    // following logic
                    return Err(TowerError::TooOldTower(
                        last_voted_slot,
                        slot_history.oldest(),
                    ));
                }

                self.adjust_lockouts_with_slot_history(slot_history)?;
                self.initialize_root(replayed_root);
            } else {
                // This should never occur under normal operation.
                // While this validator's voting is suspended this way,
                // suspended_decision_due_to_major_unsynced_ledger() will be also touched.
                let message = format!(
                    "For some reason, we're REPROCESSING slots which has already been \
                     voted and ROOTED by us; \
                     VOTING will be SUSPENDED UNTIL {last_voted_slot}!",
                );
                error!("{}", message);
                datapoint_error!("tower_error", ("error", message, String));

                // Let's pass-through adjust_lockouts_with_slot_history just for sanitization,
                // using a synthesized SlotHistory.

                let mut warped_slot_history = (*slot_history).clone();
                // Blockstore doesn't have the tower_root slot because of
                // (replayed_root < tower_root) in this else clause, meaning the tower is from
                // the future from the view of blockstore.
                // Pretend the blockstore has the future tower_root to anchor exactly with that
                // slot by adding tower_root to a slot history. The added slot will be newer
                // than all slots in the slot history (remember tower_root > replayed_root),
                // satisfying the slot history invariant.
                // Thus, the whole process will be safe as well because tower_root exists
                // within both tower and slot history, guaranteeing the success of adjustment
                // and retaining all of future votes correctly while sanitizing.
                warped_slot_history.add(tower_root);

                self.adjust_lockouts_with_slot_history(&warped_slot_history)?;
                // don't update root; future tower's root should be kept across validator
                // restarts to continue to show the scary messages at restarts until the next
                // voting.
            }
        } else {
            // This else clause is for newly created tower.
            // initialize_lockouts_from_bank() should ensure the following invariant,
            // otherwise we're screwing something up.
            assert_eq!(tower_root, replayed_root);
        }

        Ok(self)
    }

    fn adjust_lockouts_with_slot_history(&mut self, slot_history: &SlotHistory) -> Result<()> {
        let tower_root = self.root();
        // retained slots will be consisted only from divergent slots
        let mut retain_flags_for_each_vote_in_reverse: Vec<_> =
            Vec::with_capacity(self.vote_state.votes.len());

        let mut still_in_future = true;
        let mut past_outside_history = false;
        let mut checked_slot = None;
        let mut anchored_slot = None;

        let mut slots_in_tower = vec![tower_root];
        slots_in_tower.extend(self.voted_slots());

        // iterate over votes + root (if any) in the newest => oldest order
        // bail out early if bad condition is found
        for slot_in_tower in slots_in_tower.iter().rev() {
            let check = slot_history.check(*slot_in_tower);

            if anchored_slot.is_none() && check == Check::Found {
                anchored_slot = Some(*slot_in_tower);
            } else if anchored_slot.is_some() && check == Check::NotFound {
                // this can't happen unless we're fed with bogus snapshot
                return Err(TowerError::FatallyInconsistent("diverged ancestor?"));
            }

            if still_in_future && check != Check::Future {
                still_in_future = false;
            } else if !still_in_future && check == Check::Future {
                // really odd cases: bad ordered votes?
                return Err(TowerError::FatallyInconsistent("time warped?"));
            }
            if !past_outside_history && check == Check::TooOld {
                past_outside_history = true;
            } else if past_outside_history && check != Check::TooOld {
                // really odd cases: bad ordered votes?
                return Err(TowerError::FatallyInconsistent(
                    "not too old once after got too old?",
                ));
            }

            if let Some(checked_slot) = checked_slot {
                // This is really special, only if tower is initialized and contains
                // a vote for the root, the root slot can repeat only once
                let voting_for_root =
                    *slot_in_tower == checked_slot && *slot_in_tower == tower_root;

                if !voting_for_root {
                    // Unless we're voting since genesis, slots_in_tower must always be older than last checked_slot
                    // including all vote slot and the root slot.
                    assert!(
                        *slot_in_tower < checked_slot,
                        "slot_in_tower({}) < checked_slot({})",
                        *slot_in_tower,
                        checked_slot
                    );
                }
            }

            checked_slot = Some(*slot_in_tower);

            retain_flags_for_each_vote_in_reverse.push(anchored_slot.is_none());
        }

        // Check for errors if not anchored
        info!("adjusted tower's anchored slot: {:?}", anchored_slot);
        if anchored_slot.is_none() {
            // this error really shouldn't happen unless ledger/tower is corrupted
            return Err(TowerError::FatallyInconsistent(
                "no common slot for rooted tower",
            ));
        }

        assert_eq!(
            slots_in_tower.len(),
            retain_flags_for_each_vote_in_reverse.len()
        );
        // pop for the tower root
        retain_flags_for_each_vote_in_reverse.pop();
        let mut retain_flags_for_each_vote =
            retain_flags_for_each_vote_in_reverse.into_iter().rev();

        let original_votes_len = self.vote_state.votes.len();
        self.initialize_lockouts(move |_| retain_flags_for_each_vote.next().unwrap());

        if self.vote_state.votes.is_empty() {
            info!("All restored votes were behind; resetting root_slot and last_vote in tower!");
            // we might not have banks for those votes so just reset.
            // That's because the votes may well past replayed_root
            self.last_vote = VoteTransaction::from(Vote::default());
        } else {
            info!(
                "{} restored votes (out of {}) were on different fork or are upcoming votes on unrooted slots: {:?}!",
                self.voted_slots().len(),
                original_votes_len,
                self.voted_slots()
            );

            assert_eq!(self.last_voted_slot(), self.voted_slots().last().copied());
            self.stray_restored_slot = self.last_vote.last_voted_slot()
        }

        Ok(())
    }

    fn initialize_lockouts_from_bank(
        &mut self,
        vote_account_pubkey: &Pubkey,
        root: Slot,
        bank: &Bank,
    ) {
        if let Some(vote_account) = bank.get_vote_account(vote_account_pubkey) {
            self.vote_state = vote_account
                .vote_state()
                .cloned()
                .expect("vote_account isn't a VoteState?");
            self.initialize_root(root);
            self.initialize_lockouts(|v| v.slot() > root);
            trace!(
                "Lockouts in tower for {} is initialized using bank {}",
                self.vote_state.node_pubkey,
                bank.slot(),
            );
        } else {
            self.initialize_root(root);
            info!(
                "vote account({}) not found in bank (slot={})",
                vote_account_pubkey,
                bank.slot()
            );
        }
    }

    fn initialize_lockouts<F: FnMut(&LandedVote) -> bool>(&mut self, should_retain: F) {
        self.vote_state.votes.retain(should_retain);
    }

    // Updating root is needed to correctly restore from newly-saved tower for the next
    // boot
    fn initialize_root(&mut self, root: Slot) {
        self.vote_state.root_slot = Some(root);
    }

    pub fn save(&self, tower_storage: &dyn TowerStorage, node_keypair: &Keypair) -> Result<()> {
        let saved_tower = SavedTower::new(self, node_keypair)?;
        tower_storage.store(&SavedTowerVersions::from(saved_tower))?;
        Ok(())
    }

    pub fn restore(tower_storage: &dyn TowerStorage, node_pubkey: &Pubkey) -> Result<Self> {
        tower_storage.load(node_pubkey)
    }
}

#[derive(Error, Debug)]
pub enum TowerError {
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization Error: {0}")]
    SerializeError(#[from] bincode::Error),

    #[error("The signature on the saved tower is invalid")]
    InvalidSignature,

    #[error("The tower does not match this validator: {0}")]
    WrongTower(String),

    #[error(
        "The tower is too old: \
        newest slot in tower ({0}) << oldest slot in available history ({1})"
    )]
    TooOldTower(Slot, Slot),

    #[error("The tower is fatally inconsistent with blockstore: {0}")]
    FatallyInconsistent(&'static str),

    #[error("The tower is useless because of new hard fork: {0}")]
    HardFork(Slot),
}

// Tower1_14_11 is the persisted data format for the Tower, decoupling it from VoteState::Current
// From Tower1_14_11 to Tower is not implemented because it is not an expected conversion
#[allow(clippy::from_over_into)]
impl Into<Tower1_14_11> for Tower {
    fn into(self) -> Tower1_14_11 {
        Tower1_14_11 {
            node_pubkey: self.node_pubkey,
            threshold_depth: self.threshold_depth,
            threshold_size: self.threshold_size,
            vote_state: VoteState1_14_11::from(self.vote_state.clone()),
            last_vote: self.last_vote.clone(),
            last_vote_tx_blockhash: self.last_vote_tx_blockhash,
            last_timestamp: self.last_timestamp,
            stray_restored_slot: self.stray_restored_slot,
            last_switch_threshold_check: self.last_switch_threshold_check,
        }
    }
}

impl TowerError {
    pub fn is_file_missing(&self) -> bool {
        if let TowerError::IoError(io_err) = &self {
            io_err.kind() == std::io::ErrorKind::NotFound
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub enum ExternalRootSource {
    Tower(Slot),
    HardFork(Slot),
}

impl ExternalRootSource {
    fn root(&self) -> Slot {
        match self {
            ExternalRootSource::Tower(slot) => *slot,
            ExternalRootSource::HardFork(slot) => *slot,
        }
    }
}

// Given an untimely crash, tower may have roots that are not reflected in blockstore,
// or the reverse of this.
// That's because we don't impose any ordering guarantee or any kind of write barriers
// between tower (plain old POSIX fs calls) and blockstore (through RocksDB), when
// `ReplayState::handle_votable_bank()` saves tower before setting blockstore roots.
pub fn reconcile_blockstore_roots_with_external_source(
    external_source: ExternalRootSource,
    blockstore: &Blockstore,
    // blockstore.last_root() might have been updated already.
    // so take a &mut param both to input (and output iff we update root)
    last_blockstore_root: &mut Slot,
) -> blockstore_db::Result<()> {
    let external_root = external_source.root();
    if *last_blockstore_root < external_root {
        // Ensure external_root itself to exist and be marked as rooted in the blockstore
        // in addition to its ancestors.
        let new_roots: Vec<_> = AncestorIterator::new_inclusive(external_root, blockstore)
            .take_while(|current| match current.cmp(last_blockstore_root) {
                Ordering::Greater => true,
                Ordering::Equal => false,
                Ordering::Less => panic!(
                    "last_blockstore_root({last_blockstore_root}) is skipped while traversing \
                     blockstore (currently at {current}) from external root ({external_source:?})!?",
                ),
            })
            .collect();
        if !new_roots.is_empty() {
            info!(
                "Reconciling slots as root based on external root: {:?} (external: {:?}, blockstore: {})",
                new_roots, external_source, last_blockstore_root
            );

            // Unfortunately, we can't supply duplicate-confirmed hashes,
            // because it can't be guaranteed to be able to replay these slots
            // under this code-path's limited condition (i.e.  those shreds
            // might not be available, etc...) also correctly overcoming this
            // limitation is hard...
            blockstore.mark_slots_as_if_rooted_normally_at_startup(
                new_roots.into_iter().map(|root| (root, None)).collect(),
                false,
            )?;

            // Update the caller-managed state of last root in blockstore.
            // Repeated calls of this function should result in a no-op for
            // the range of `new_roots`.
            *last_blockstore_root = blockstore.last_root();
        } else {
            // This indicates we're in bad state; but still don't panic here.
            // That's because we might have a chance of recovering properly with
            // newer snapshot.
            warn!(
                "Couldn't find any ancestor slots from external source ({:?}) \
                 towards blockstore root ({}); blockstore pruned or only \
                 tower moved into new ledger or just hard fork?",
                external_source, last_blockstore_root,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::{
            consensus::{
                fork_choice::ForkChoice, heaviest_subtree_fork_choice::SlotHashKey,
                tower_storage::FileTowerStorage,
            },
            replay_stage::HeaviestForkFailures,
            vote_simulator::VoteSimulator,
        },
        itertools::Itertools,
        solana_ledger::{blockstore::make_slot_entries, get_tmp_ledger_path},
        solana_runtime::bank::Bank,
        solana_sdk::{
            account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
            clock::Slot,
            hash::Hash,
            pubkey::Pubkey,
            signature::Signer,
            slot_history::SlotHistory,
        },
        solana_vote::vote_account::VoteAccount,
        solana_vote_program::vote_state::{Vote, VoteStateVersions, MAX_LOCKOUT_HISTORY},
        std::{
            collections::{HashMap, VecDeque},
            fs::{remove_file, OpenOptions},
            io::{Read, Seek, SeekFrom, Write},
            path::PathBuf,
            sync::Arc,
        },
        tempfile::TempDir,
        trees::tr,
    };

    fn gen_stakes(stake_votes: &[(u64, &[u64])]) -> VoteAccountsHashMap {
        stake_votes
            .iter()
            .map(|(lamports, votes)| {
                let mut account = AccountSharedData::from(Account {
                    data: vec![0; VoteState::size_of()],
                    lamports: *lamports,
                    owner: solana_vote_program::id(),
                    ..Account::default()
                });
                let mut vote_state = VoteState::default();
                for slot in *votes {
                    process_slot_vote_unchecked(&mut vote_state, *slot);
                }
                VoteState::serialize(
                    &VoteStateVersions::new_current(vote_state),
                    account.data_as_mut_slice(),
                )
                .expect("serialize state");
                (
                    solana_sdk::pubkey::new_rand(),
                    (*lamports, VoteAccount::try_from(account).unwrap()),
                )
            })
            .collect()
    }

    #[test]
    fn test_to_vote_instruction() {
        let vote = Vote::default();
        let mut decision = SwitchForkDecision::FailedSwitchThreshold(0, 1);
        assert!(decision
            .to_vote_instruction(
                VoteTransaction::from(vote.clone()),
                &Pubkey::default(),
                &Pubkey::default()
            )
            .is_none());

        decision = SwitchForkDecision::FailedSwitchDuplicateRollback(0);
        assert!(decision
            .to_vote_instruction(
                VoteTransaction::from(vote.clone()),
                &Pubkey::default(),
                &Pubkey::default()
            )
            .is_none());

        decision = SwitchForkDecision::SameFork;
        assert_eq!(
            decision.to_vote_instruction(
                VoteTransaction::from(vote.clone()),
                &Pubkey::default(),
                &Pubkey::default()
            ),
            Some(vote_instruction::vote(
                &Pubkey::default(),
                &Pubkey::default(),
                vote.clone(),
            ))
        );

        decision = SwitchForkDecision::SwitchProof(Hash::default());
        assert_eq!(
            decision.to_vote_instruction(
                VoteTransaction::from(vote.clone()),
                &Pubkey::default(),
                &Pubkey::default()
            ),
            Some(vote_instruction::vote_switch(
                &Pubkey::default(),
                &Pubkey::default(),
                vote,
                Hash::default()
            ))
        );
    }

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
    fn test_collect_vote_lockouts_sums() {
        //two accounts voting for slot 0 with 1 token staked
        let accounts = gen_stakes(&[(1, &[0]), (1, &[0])]);
        let account_latest_votes: Vec<(Pubkey, SlotHashKey)> = accounts
            .iter()
            .sorted_by_key(|(pk, _)| *pk)
            .map(|(pubkey, _)| (*pubkey, (0, Hash::default())))
            .collect();

        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        let mut latest_validator_votes_for_frozen_banks =
            LatestValidatorVotesForFrozenBanks::default();
        let ComputedBankState {
            voted_stakes,
            total_stake,
            bank_weight,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            1,
            &accounts,
            &ancestors,
            |_| Some(Hash::default()),
            &mut latest_validator_votes_for_frozen_banks,
        );
        assert_eq!(voted_stakes[&0], 2);
        assert_eq!(total_stake, 2);
        let mut new_votes = latest_validator_votes_for_frozen_banks.take_votes_dirty_set(0);
        new_votes.sort();
        assert_eq!(new_votes, account_latest_votes);

        // Each account has 1 vote in it. After simulating a vote in collect_vote_lockouts,
        // the account will have 2 votes, with lockout 2 + 4 = 6. So expected weight for
        assert_eq!(bank_weight, 12)
    }

    #[test]
    fn test_collect_vote_lockouts_root() {
        let votes: Vec<u64> = (0..MAX_LOCKOUT_HISTORY as u64).collect();
        //two accounts voting for slots 0..MAX_LOCKOUT_HISTORY with 1 token staked
        let accounts = gen_stakes(&[(1, &votes), (1, &votes)]);
        let account_latest_votes: Vec<(Pubkey, SlotHashKey)> = accounts
            .iter()
            .sorted_by_key(|(pk, _)| *pk)
            .map(|(pubkey, _)| {
                (
                    *pubkey,
                    ((MAX_LOCKOUT_HISTORY - 1) as Slot, Hash::default()),
                )
            })
            .collect();
        let mut tower = Tower::new_for_tests(0, 0.67);
        let mut ancestors = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            tower.record_vote(i as u64, Hash::default());
            ancestors.insert(i as u64, (0..i as u64).collect());
        }
        let root = Lockout::new_with_confirmation_count(0, MAX_LOCKOUT_HISTORY as u32);
        let root_weight = root.lockout() as u128;
        let vote_account_expected_weight = tower
            .vote_state
            .votes
            .iter()
            .map(|v| v.lockout.lockout() as u128)
            .sum::<u128>()
            + root_weight;
        let expected_bank_weight = 2 * vote_account_expected_weight;
        assert_eq!(tower.vote_state.root_slot, Some(0));
        let mut latest_validator_votes_for_frozen_banks =
            LatestValidatorVotesForFrozenBanks::default();
        let ComputedBankState {
            voted_stakes,
            bank_weight,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            MAX_LOCKOUT_HISTORY as u64,
            &accounts,
            &ancestors,
            |_| Some(Hash::default()),
            &mut latest_validator_votes_for_frozen_banks,
        );
        for i in 0..MAX_LOCKOUT_HISTORY {
            assert_eq!(voted_stakes[&(i as u64)], 2);
        }

        // should be the sum of all the weights for root
        assert_eq!(bank_weight, expected_bank_weight);
        let mut new_votes =
            latest_validator_votes_for_frozen_banks.take_votes_dirty_set(root.slot());
        new_votes.sort();
        assert_eq!(new_votes, account_latest_votes);
    }

    #[test]
    fn test_check_vote_threshold_without_votes() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1)].into_iter().collect();
        assert!(tower.check_vote_stake_threshold(0, &stakes, 2).passed());
    }

    #[test]
    fn test_check_vote_threshold_no_skip_lockout_with_new_root() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(4, 0.67);
        let mut stakes = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY as u64 + 1) {
            stakes.insert(i, 1);
            tower.record_vote(i, Hash::default());
        }
        assert!(!tower
            .check_vote_stake_threshold(MAX_LOCKOUT_HISTORY as u64 + 1, &stakes, 2,)
            .passed());
    }

    #[test]
    fn test_is_slot_confirmed_not_enough_stake_failure() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1)].into_iter().collect();
        assert!(!tower.is_slot_confirmed(0, &stakes, 2));
    }

    #[test]
    fn test_is_slot_confirmed_unknown_slot() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = HashMap::new();
        assert!(!tower.is_slot_confirmed(0, &stakes, 2));
    }

    #[test]
    fn test_is_slot_confirmed_pass() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 2)].into_iter().collect();
        assert!(tower.is_slot_confirmed(0, &stakes, 2));
    }

    #[test]
    fn test_is_locked_out_empty() {
        let tower = Tower::new_for_tests(0, 0.67);
        let ancestors = HashSet::from([0]);
        assert!(!tower.is_locked_out(1, &ancestors));
    }

    #[test]
    fn test_is_locked_out_root_slot_child_pass() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors: HashSet<Slot> = vec![0].into_iter().collect();
        tower.vote_state.root_slot = Some(0);
        assert!(!tower.is_locked_out(1, &ancestors));
    }

    #[test]
    fn test_is_locked_out_root_slot_sibling_fail() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors: HashSet<Slot> = vec![0].into_iter().collect();
        tower.vote_state.root_slot = Some(0);
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(2, &ancestors));
    }

    #[test]
    fn test_check_already_voted() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        tower.record_vote(0, Hash::default());
        assert!(tower.has_voted(0));
        assert!(!tower.has_voted(1));
    }

    #[test]
    fn test_check_recent_slot() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        assert!(tower.is_recent(1));
        assert!(tower.is_recent(32));
        for i in 0..64 {
            tower.record_vote(i, Hash::default());
        }
        assert!(!tower.is_recent(0));
        assert!(!tower.is_recent(32));
        assert!(!tower.is_recent(63));
        assert!(tower.is_recent(65));
    }

    #[test]
    fn test_is_locked_out_double_vote() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors: HashSet<Slot> = vec![0].into_iter().collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(0, &ancestors));
    }

    #[test]
    fn test_is_locked_out_child() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors: HashSet<Slot> = vec![0].into_iter().collect();
        tower.record_vote(0, Hash::default());
        assert!(!tower.is_locked_out(1, &ancestors));
    }

    #[test]
    fn test_is_locked_out_sibling() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors: HashSet<Slot> = vec![0].into_iter().collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(2, &ancestors));
    }

    #[test]
    fn test_is_locked_out_last_vote_expired() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors: HashSet<Slot> = vec![0].into_iter().collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(!tower.is_locked_out(4, &ancestors));
        tower.record_vote(4, Hash::default());
        assert_eq!(tower.vote_state.votes[0].slot(), 0);
        assert_eq!(tower.vote_state.votes[0].confirmation_count(), 2);
        assert_eq!(tower.vote_state.votes[1].slot(), 4);
        assert_eq!(tower.vote_state.votes[1].confirmation_count(), 1);
    }

    #[test]
    fn test_check_vote_threshold_below_threshold() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        assert!(!tower.check_vote_stake_threshold(1, &stakes, 2).passed());
    }
    #[test]
    fn test_check_vote_threshold_above_threshold() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 2)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        assert!(tower.check_vote_stake_threshold(1, &stakes, 2).passed());
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_after_pop() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 2)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        assert!(tower.check_vote_stake_threshold(6, &stakes, 2).passed());
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_no_stake() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = HashMap::new();
        tower.record_vote(0, Hash::default());
        assert!(!tower.check_vote_stake_threshold(1, &stakes, 2).passed());
    }

    #[test]
    fn test_check_vote_threshold_lockouts_not_updated() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1), (1, 2)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        assert!(tower.check_vote_stake_threshold(6, &stakes, 2,).passed());
    }

    #[test]
    fn test_stake_is_updated_for_entire_branch() {
        let mut voted_stakes = HashMap::new();
        let account = AccountSharedData::from(Account {
            lamports: 1,
            ..Account::default()
        });
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let ancestors: HashMap<u64, HashSet<u64>> = [(2u64, set)].iter().cloned().collect();
        Tower::update_ancestor_voted_stakes(&mut voted_stakes, 2, account.lamports(), &ancestors);
        assert_eq!(voted_stakes[&0], 1);
        assert_eq!(voted_stakes[&1], 1);
        assert_eq!(voted_stakes[&2], 1);
    }

    #[test]
    fn test_check_vote_threshold_forks() {
        // Create the ancestor relationships
        let ancestors = (0..=(VOTE_THRESHOLD_DEPTH + 1) as u64)
            .map(|slot| {
                let slot_parents: HashSet<_> = (0..slot).collect();
                (slot, slot_parents)
            })
            .collect();

        // Create votes such that
        // 1) 3/4 of the stake has voted on slot: VOTE_THRESHOLD_DEPTH - 2, lockout: 2
        // 2) 1/4 of the stake has voted on slot: VOTE_THRESHOLD_DEPTH, lockout: 2^9
        let total_stake = 4;
        let threshold_size = 0.67;
        let threshold_stake = (f64::ceil(total_stake as f64 * threshold_size)) as u64;
        let tower_votes: Vec<Slot> = (0..VOTE_THRESHOLD_DEPTH as u64).collect();
        let accounts = gen_stakes(&[
            (threshold_stake, &[(VOTE_THRESHOLD_DEPTH - 2) as u64]),
            (total_stake - threshold_stake, &tower_votes[..]),
        ]);

        // Initialize tower
        let mut tower = Tower::new_for_tests(VOTE_THRESHOLD_DEPTH, threshold_size);

        // CASE 1: Record the first VOTE_THRESHOLD tower votes for fork 2. We want to
        // evaluate a vote on slot VOTE_THRESHOLD_DEPTH. The nth most recent vote should be
        // for slot 0, which is common to all account vote states, so we should pass the
        // threshold check
        let vote_to_evaluate = VOTE_THRESHOLD_DEPTH as u64;
        for vote in &tower_votes {
            tower.record_vote(*vote, Hash::default());
        }
        let ComputedBankState {
            voted_stakes,
            total_stake,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            vote_to_evaluate,
            &accounts,
            &ancestors,
            |_| None,
            &mut LatestValidatorVotesForFrozenBanks::default(),
        );
        assert!(tower
            .check_vote_stake_threshold(vote_to_evaluate, &voted_stakes, total_stake,)
            .passed());

        // CASE 2: Now we want to evaluate a vote for slot VOTE_THRESHOLD_DEPTH + 1. This slot
        // will expire the vote in one of the vote accounts, so we should have insufficient
        // stake to pass the threshold
        let vote_to_evaluate = VOTE_THRESHOLD_DEPTH as u64 + 1;
        let ComputedBankState {
            voted_stakes,
            total_stake,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            vote_to_evaluate,
            &accounts,
            &ancestors,
            |_| None,
            &mut LatestValidatorVotesForFrozenBanks::default(),
        );
        assert!(!tower
            .check_vote_stake_threshold(vote_to_evaluate, &voted_stakes, total_stake,)
            .passed());
    }

    fn vote_and_check_recent(num_votes: usize) {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let slots = if num_votes > 0 {
            { 0..num_votes }
                .map(|i| {
                    Lockout::new_with_confirmation_count(i as Slot, (num_votes as u32) - (i as u32))
                })
                .collect()
        } else {
            vec![]
        };
        let mut expected = VoteStateUpdate::new(
            VecDeque::from(slots),
            if num_votes > 0 { Some(0) } else { None },
            Hash::default(),
        );
        for i in 0..num_votes {
            tower.record_vote(i as u64, Hash::default());
        }

        expected.timestamp = tower.last_vote.timestamp();
        assert_eq!(VoteTransaction::from(expected), tower.last_vote)
    }

    #[test]
    fn test_recent_votes_full() {
        vote_and_check_recent(MAX_LOCKOUT_HISTORY)
    }

    #[test]
    fn test_recent_votes_empty() {
        vote_and_check_recent(0)
    }

    #[test]
    fn test_recent_votes_exact() {
        vote_and_check_recent(5)
    }

    #[test]
    fn test_maybe_timestamp() {
        let mut tower = Tower::default();
        assert!(tower.maybe_timestamp(0).is_some());
        assert!(tower.maybe_timestamp(1).is_some());
        assert!(tower.maybe_timestamp(0).is_none()); // Refuse to timestamp an older slot
        assert!(tower.maybe_timestamp(1).is_none()); // Refuse to timestamp the same slot twice

        tower.last_timestamp.timestamp -= 1; // Move last_timestamp into the past
        assert!(tower.maybe_timestamp(2).is_some()); // slot 2 gets a timestamp

        tower.last_timestamp.timestamp += 1_000_000; // Move last_timestamp well into the future
        assert!(tower.maybe_timestamp(3).is_none()); // slot 3 gets no timestamp
    }

    #[test]
    fn test_refresh_last_vote_timestamp() {
        let mut tower = Tower::default();

        // Tower has no vote or timestamp
        tower.last_vote.set_timestamp(None);
        tower.refresh_last_vote_timestamp(5);
        assert_eq!(tower.last_vote.timestamp(), None);
        assert_eq!(tower.last_timestamp.slot, 0);
        assert_eq!(tower.last_timestamp.timestamp, 0);

        // Tower has vote no timestamp, but is greater than heaviest_bank
        tower.last_vote =
            VoteTransaction::from(VoteStateUpdate::from(vec![(0, 3), (1, 2), (6, 1)]));
        assert_eq!(tower.last_vote.timestamp(), None);
        tower.refresh_last_vote_timestamp(5);
        assert_eq!(tower.last_vote.timestamp(), None);
        assert_eq!(tower.last_timestamp.slot, 0);
        assert_eq!(tower.last_timestamp.timestamp, 0);

        // Tower has vote with no timestamp
        tower.last_vote =
            VoteTransaction::from(VoteStateUpdate::from(vec![(0, 3), (1, 2), (2, 1)]));
        assert_eq!(tower.last_vote.timestamp(), None);
        tower.refresh_last_vote_timestamp(5);
        assert_eq!(tower.last_vote.timestamp(), Some(1));
        assert_eq!(tower.last_timestamp.slot, 2);
        assert_eq!(tower.last_timestamp.timestamp, 1);

        // Vote has timestamp
        tower.last_vote =
            VoteTransaction::from(VoteStateUpdate::from(vec![(0, 3), (1, 2), (2, 1)]));
        tower.refresh_last_vote_timestamp(5);
        assert_eq!(tower.last_vote.timestamp(), Some(2));
        assert_eq!(tower.last_timestamp.slot, 2);
        assert_eq!(tower.last_timestamp.timestamp, 2);
    }

    fn run_test_load_tower_snapshot<F, G>(
        modify_original: F,
        modify_serialized: G,
    ) -> (Tower, Result<Tower>)
    where
        F: Fn(&mut Tower, &Pubkey),
        G: Fn(&PathBuf),
    {
        let tower_path = TempDir::new().unwrap();
        let identity_keypair = Arc::new(Keypair::new());
        let node_pubkey = identity_keypair.pubkey();

        // Use values that will not match the default derived from BankForks
        let mut tower = Tower::new_for_tests(10, 0.9);

        let tower_storage = FileTowerStorage::new(tower_path.path().to_path_buf());

        modify_original(&mut tower, &node_pubkey);

        tower.save(&tower_storage, &identity_keypair).unwrap();
        modify_serialized(&tower_storage.filename(&node_pubkey));
        let loaded = Tower::restore(&tower_storage, &node_pubkey);

        (tower, loaded)
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
    fn test_load_tower_ok() {
        let (tower, loaded) =
            run_test_load_tower_snapshot(|tower, pubkey| tower.node_pubkey = *pubkey, |_| ());
        let loaded = loaded.unwrap();
        assert_eq!(loaded, tower);
        assert_eq!(tower.threshold_depth, 10);
        assert!((tower.threshold_size - 0.9_f64).abs() < f64::EPSILON);
        assert_eq!(loaded.threshold_depth, 10);
        assert!((loaded.threshold_size - 0.9_f64).abs() < f64::EPSILON);
    }

    #[test]
    fn test_load_tower_wrong_identity() {
        let identity_keypair = Arc::new(Keypair::new());
        let tower = Tower::default();
        let tower_storage = FileTowerStorage::default();
        assert_matches!(
            tower.save(&tower_storage, &identity_keypair),
            Err(TowerError::WrongTower(_))
        )
    }

    #[test]
    fn test_load_tower_invalid_signature() {
        let (_, loaded) = run_test_load_tower_snapshot(
            |tower, pubkey| tower.node_pubkey = *pubkey,
            |path| {
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .unwrap();
                // 4 is the offset into SavedTowerVersions for the signature
                assert_eq!(file.seek(SeekFrom::Start(4)).unwrap(), 4);
                let mut buf = [0u8];
                assert_eq!(file.read(&mut buf).unwrap(), 1);
                buf[0] = !buf[0];
                assert_eq!(file.seek(SeekFrom::Start(4)).unwrap(), 4);
                assert_eq!(file.write(&buf).unwrap(), 1);
            },
        );
        assert_matches!(loaded, Err(TowerError::InvalidSignature))
    }

    #[test]
    fn test_load_tower_deser_failure() {
        let (_, loaded) = run_test_load_tower_snapshot(
            |tower, pubkey| tower.node_pubkey = *pubkey,
            |path| {
                OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(path)
                    .unwrap_or_else(|_| panic!("Failed to truncate file: {path:?}"));
            },
        );
        assert_matches!(loaded, Err(TowerError::SerializeError(_)))
    }

    #[test]
    fn test_load_tower_missing() {
        let (_, loaded) = run_test_load_tower_snapshot(
            |tower, pubkey| tower.node_pubkey = *pubkey,
            |path| {
                remove_file(path).unwrap();
            },
        );
        assert_matches!(loaded, Err(TowerError::IoError(_)))
    }

    #[test]
    fn test_reconcile_blockstore_roots_with_tower_normal() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(1, 0, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(3, 1, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(4, 1, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            assert!(!blockstore.is_root(0));
            assert!(!blockstore.is_root(1));
            assert!(!blockstore.is_root(3));
            assert!(!blockstore.is_root(4));

            let mut tower = Tower::default();
            tower.vote_state.root_slot = Some(4);
            reconcile_blockstore_roots_with_external_source(
                ExternalRootSource::Tower(tower.root()),
                &blockstore,
                &mut blockstore.last_root(),
            )
            .unwrap();

            assert!(!blockstore.is_root(0));
            assert!(blockstore.is_root(1));
            assert!(!blockstore.is_root(3));
            assert!(blockstore.is_root(4));
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    #[should_panic(expected = "last_blockstore_root(3) is skipped while \
                               traversing blockstore (currently at 1) from \
                               external root (Tower(4))!?")]
    fn test_reconcile_blockstore_roots_with_tower_panic_no_common_root() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(1, 0, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(3, 1, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(4, 1, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            blockstore.set_roots(std::iter::once(&3)).unwrap();
            assert!(!blockstore.is_root(0));
            assert!(!blockstore.is_root(1));
            assert!(blockstore.is_root(3));
            assert!(!blockstore.is_root(4));

            let mut tower = Tower::default();
            tower.vote_state.root_slot = Some(4);
            reconcile_blockstore_roots_with_external_source(
                ExternalRootSource::Tower(tower.root()),
                &blockstore,
                &mut blockstore.last_root(),
            )
            .unwrap();
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_reconcile_blockstore_roots_with_tower_nop_no_parent() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(1, 0, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(3, 1, 42, /*merkle_variant:*/ true);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            assert!(!blockstore.is_root(0));
            assert!(!blockstore.is_root(1));
            assert!(!blockstore.is_root(3));

            let mut tower = Tower::default();
            tower.vote_state.root_slot = Some(4);
            assert_eq!(blockstore.last_root(), 0);
            reconcile_blockstore_roots_with_external_source(
                ExternalRootSource::Tower(tower.root()),
                &blockstore,
                &mut blockstore.last_root(),
            )
            .unwrap();
            assert_eq!(blockstore.last_root(), 0);
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_adjust_lockouts_after_replay_future_slots() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        tower.record_vote(3, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);

        let replayed_root_slot = 1;
        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![2, 3]);
        assert_eq!(tower.root(), replayed_root_slot);

        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();
        assert_eq!(tower.voted_slots(), vec![2, 3]);
        assert_eq!(tower.root(), replayed_root_slot);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_not_found_slots() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        tower.record_vote(3, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);
        slot_history.add(4);

        let replayed_root_slot = 4;
        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![2, 3]);
        assert_eq!(tower.root(), replayed_root_slot);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_all_rooted_with_no_too_old() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);
        slot_history.add(2);
        slot_history.add(3);
        slot_history.add(4);
        slot_history.add(5);

        let replayed_root_slot = 5;
        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![] as Vec<Slot>);
        assert_eq!(tower.root(), replayed_root_slot);
        assert_eq!(tower.stray_restored_slot, None);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_all_rooted_with_too_old() {
        use solana_sdk::slot_history::MAX_ENTRIES;

        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);
        slot_history.add(2);
        slot_history.add(MAX_ENTRIES);

        tower = tower
            .adjust_lockouts_after_replay(MAX_ENTRIES, &slot_history)
            .unwrap();
        assert_eq!(tower.voted_slots(), vec![] as Vec<Slot>);
        assert_eq!(tower.root(), MAX_ENTRIES);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_anchored_future_slots() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        tower.record_vote(3, Hash::default());
        tower.record_vote(4, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);
        slot_history.add(2);

        let replayed_root_slot = 2;
        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![3, 4]);
        assert_eq!(tower.root(), replayed_root_slot);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_all_not_found() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(5, Hash::default());
        tower.record_vote(6, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);
        slot_history.add(2);
        slot_history.add(7);

        let replayed_root_slot = 7;
        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![5, 6]);
        assert_eq!(tower.root(), replayed_root_slot);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_all_not_found_even_if_rooted() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.vote_state.root_slot = Some(4);
        tower.record_vote(5, Hash::default());
        tower.record_vote(6, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);
        slot_history.add(2);
        slot_history.add(7);

        let replayed_root_slot = 7;
        let result = tower.adjust_lockouts_after_replay(replayed_root_slot, &slot_history);

        assert_eq!(
            format!("{}", result.unwrap_err()),
            "The tower is fatally inconsistent with blockstore: no common slot for rooted tower"
        );
    }

    #[test]
    fn test_adjust_lockouts_after_replay_all_future_votes_only_root_found() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.vote_state.root_slot = Some(2);
        tower.record_vote(3, Hash::default());
        tower.record_vote(4, Hash::default());
        tower.record_vote(5, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(1);
        slot_history.add(2);

        let replayed_root_slot = 2;
        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![3, 4, 5]);
        assert_eq!(tower.root(), replayed_root_slot);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_empty() {
        let mut tower = Tower::new_for_tests(10, 0.9);

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);

        let replayed_root_slot = 0;
        tower = tower
            .adjust_lockouts_after_replay(replayed_root_slot, &slot_history)
            .unwrap();

        assert_eq!(tower.voted_slots(), vec![] as Vec<Slot>);
        assert_eq!(tower.root(), replayed_root_slot);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_too_old_tower() {
        use solana_sdk::slot_history::MAX_ENTRIES;

        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(0, Hash::default());

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(MAX_ENTRIES);

        let result = tower.adjust_lockouts_after_replay(MAX_ENTRIES, &slot_history);
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "The tower is too old: newest slot in tower (0) << oldest slot in available history (1)"
        );
    }

    #[test]
    fn test_adjust_lockouts_after_replay_time_warped() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(1)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(0)));
        let vote = Vote::new(vec![0], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);

        let result = tower.adjust_lockouts_after_replay(0, &slot_history);
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "The tower is fatally inconsistent with blockstore: time warped?"
        );
    }

    #[test]
    fn test_adjust_lockouts_after_replay_diverged_ancestor() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(1)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(2)));
        let vote = Vote::new(vec![2], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(2);

        let result = tower.adjust_lockouts_after_replay(2, &slot_history);
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "The tower is fatally inconsistent with blockstore: diverged ancestor?"
        );
    }

    #[test]
    fn test_adjust_lockouts_after_replay_out_of_order() {
        use solana_sdk::slot_history::MAX_ENTRIES;

        let mut tower = Tower::new_for_tests(10, 0.9);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(MAX_ENTRIES - 1)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(0)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(1)));
        let vote = Vote::new(vec![1], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);

        let mut slot_history = SlotHistory::default();
        slot_history.add(MAX_ENTRIES);

        let result = tower.adjust_lockouts_after_replay(MAX_ENTRIES, &slot_history);
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "The tower is fatally inconsistent with blockstore: not too old once after got too old?"
        );
    }

    #[test]
    #[should_panic(expected = "slot_in_tower(2) < checked_slot(1)")]
    fn test_adjust_lockouts_after_replay_reversed_votes() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(2)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(1)));
        let vote = Vote::new(vec![1], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(2);

        tower
            .adjust_lockouts_after_replay(2, &slot_history)
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "slot_in_tower(3) < checked_slot(3)")]
    fn test_adjust_lockouts_after_replay_repeated_non_root_votes() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(2)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(3)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(3)));
        let vote = Vote::new(vec![3], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(2);

        tower
            .adjust_lockouts_after_replay(2, &slot_history)
            .unwrap();
    }

    #[test]
    fn test_adjust_lockouts_after_replay_vote_on_root() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.vote_state.root_slot = Some(42);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(42)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(43)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(44)));
        let vote = Vote::new(vec![44], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);

        let mut slot_history = SlotHistory::default();
        slot_history.add(42);

        let tower = tower.adjust_lockouts_after_replay(42, &slot_history);
        assert_eq!(tower.unwrap().voted_slots(), [43, 44]);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_vote_on_genesis() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(0)));
        let vote = Vote::new(vec![0], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);

        assert!(tower.adjust_lockouts_after_replay(0, &slot_history).is_ok());
    }

    #[test]
    fn test_adjust_lockouts_after_replay_future_tower() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(13)));
        tower
            .vote_state
            .votes
            .push_back(LandedVote::from(Lockout::new(14)));
        let vote = Vote::new(vec![14], Hash::default());
        tower.last_vote = VoteTransaction::from(vote);
        tower.initialize_root(12);

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        slot_history.add(2);

        let tower = tower
            .adjust_lockouts_after_replay(2, &slot_history)
            .unwrap();
        assert_eq!(tower.root(), 12);
        assert_eq!(tower.voted_slots(), vec![13, 14]);
        assert_eq!(tower.stray_restored_slot, Some(14));
    }

    #[test]
    fn test_default_tower_has_no_stray_last_vote() {
        let tower = Tower::default();
        assert!(!tower.is_stray_last_vote());
    }
}
