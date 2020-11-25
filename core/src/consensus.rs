use crate::{
    progress_map::{LockoutIntervals, ProgressMap},
    pubkey_references::PubkeyReferences,
};
use chrono::prelude::*;
use solana_ledger::{ancestor_iterator::AncestorIterator, blockstore::Blockstore, blockstore_db};
use solana_measure::measure::Measure;
use solana_runtime::{
    bank::Bank, bank_forks::BankForks, commitment::VOTE_THRESHOLD_SIZE,
    vote_account::ArcVoteAccount,
};
use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    hash::Hash,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    slot_history::{Check, SlotHistory},
};
use solana_vote_program::{
    vote_instruction,
    vote_state::{BlockTimestamp, Lockout, Vote, VoteState, MAX_LOCKOUT_HISTORY},
};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::BufReader,
    ops::Bound::{Included, Unbounded},
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;

#[derive(PartialEq, Clone, Debug, AbiExample)]
pub enum SwitchForkDecision {
    SwitchProof(Hash),
    SameFork,
    FailedSwitchThreshold(u64, u64),
}

impl SwitchForkDecision {
    pub fn to_vote_instruction(
        &self,
        vote: Vote,
        vote_account_pubkey: &Pubkey,
        authorized_voter_pubkey: &Pubkey,
    ) -> Option<Instruction> {
        match self {
            SwitchForkDecision::FailedSwitchThreshold(_, total_stake) => {
                assert_ne!(*total_stake, 0);
                None
            }
            SwitchForkDecision::SameFork => Some(vote_instruction::vote(
                vote_account_pubkey,
                authorized_voter_pubkey,
                vote,
            )),
            SwitchForkDecision::SwitchProof(switch_proof_hash) => {
                Some(vote_instruction::vote_switch(
                    vote_account_pubkey,
                    authorized_voter_pubkey,
                    vote,
                    *switch_proof_hash,
                ))
            }
        }
    }

    pub fn can_vote(&self) -> bool {
        !matches!(self, SwitchForkDecision::FailedSwitchThreshold(_, _))
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
    pub bank_weight: u128,
    // Tree of intervals of lockouts of the form [slot, slot + slot.lockout],
    // keyed by end of the range
    pub lockout_intervals: LockoutIntervals,
    pub pubkey_votes: Arc<PubkeyVotes>,
}

#[frozen_abi(digest = "Eay84NBbJqiMBfE7HHH2o6e51wcvoU79g8zCi5sw6uj3")]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, AbiExample)]
pub struct Tower {
    node_pubkey: Pubkey,
    threshold_depth: usize,
    threshold_size: f64,
    lockouts: VoteState,
    last_vote: Vote,
    last_timestamp: BlockTimestamp,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    tmp_path: PathBuf, // used before atomic fs::rename()
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
            lockouts: VoteState::default(),
            last_vote: Vote::default(),
            last_timestamp: BlockTimestamp::default(),
            path: PathBuf::default(),
            tmp_path: PathBuf::default(),
            stray_restored_slot: Option::default(),
            last_switch_threshold_check: Option::default(),
        };
        // VoteState::root_slot is ensured to be Some in Tower
        tower.lockouts.root_slot = Some(Slot::default());
        tower
    }
}

impl Tower {
    pub fn new(
        node_pubkey: &Pubkey,
        vote_account_pubkey: &Pubkey,
        root: Slot,
        bank: &Bank,
        path: &Path,
    ) -> Self {
        let path = Self::get_filename(&path, node_pubkey);
        let tmp_path = Self::get_tmp_filename(&path);
        let mut tower = Self {
            node_pubkey: *node_pubkey,
            path,
            tmp_path,
            ..Tower::default()
        };
        tower.initialize_lockouts_from_bank(vote_account_pubkey, root, bank);

        tower
    }

    #[cfg(test)]
    pub fn new_with_key(node_pubkey: &Pubkey) -> Self {
        Self {
            node_pubkey: *node_pubkey,
            ..Tower::default()
        }
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
        ledger_path: &Path,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
    ) -> Self {
        let root_bank = bank_forks.root_bank();
        let (_progress, heaviest_subtree_fork_choice, unlock_heaviest_subtree_fork_choice_slot) =
            crate::replay_stage::ReplayStage::initialize_progress_and_fork_choice(
                root_bank,
                bank_forks.frozen_banks().values().cloned().collect(),
                &my_pubkey,
                &vote_account,
            );
        let root = root_bank.slot();

        let heaviest_bank = if root > unlock_heaviest_subtree_fork_choice_slot {
            bank_forks
                .get(heaviest_subtree_fork_choice.best_overall_slot())
                .expect("The best overall slot must be one of `frozen_banks` which all exist in bank_forks")
                .clone()
        } else {
            Tower::find_heaviest_bank(&bank_forks, &my_pubkey).unwrap_or_else(|| root_bank.clone())
        };

        Self::new(
            &my_pubkey,
            &vote_account,
            root,
            &heaviest_bank,
            &ledger_path,
        )
    }

    pub(crate) fn collect_vote_lockouts<F>(
        node_pubkey: &Pubkey,
        bank_slot: Slot,
        vote_accounts: F,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
        all_pubkeys: &mut PubkeyReferences,
    ) -> ComputedBankState
    where
        F: IntoIterator<Item = (Pubkey, (u64, ArcVoteAccount))>,
    {
        let mut voted_stakes = HashMap::new();
        let mut total_stake = 0;
        let mut bank_weight = 0;
        // Tree of intervals of lockouts of the form [slot, slot + slot.lockout],
        // keyed by end of the range
        let mut lockout_intervals = LockoutIntervals::new();
        let mut pubkey_votes = vec![];
        for (key, (voted_stake, account)) in vote_accounts {
            if voted_stake == 0 {
                continue;
            }
            trace!("{} {} with stake {}", node_pubkey, key, voted_stake);
            let mut vote_state = match account.vote_state().as_ref() {
                Err(_) => {
                    datapoint_warn!(
                        "tower_warn",
                        (
                            "warn",
                            format!("Unable to get vote_state from account {}", key),
                            String
                        ),
                    );
                    continue;
                }
                Ok(vote_state) => vote_state.clone(),
            };
            for vote in &vote_state.votes {
                let key = all_pubkeys.get_or_insert(&key);
                lockout_intervals
                    .entry(vote.expiration_slot())
                    .or_insert_with(Vec::new)
                    .push((vote.slot, key));
            }

            if key == *node_pubkey || vote_state.node_pubkey == *node_pubkey {
                debug!("vote state {:?}", vote_state);
                debug!(
                    "observed slot {}",
                    vote_state.nth_recent_vote(0).map(|v| v.slot).unwrap_or(0) as i64
                );
                debug!("observed root {}", vote_state.root_slot.unwrap_or(0) as i64);
                datapoint_info!(
                    "tower-observed",
                    (
                        "slot",
                        vote_state.nth_recent_vote(0).map(|v| v.slot).unwrap_or(0),
                        i64
                    ),
                    ("root", vote_state.root_slot.unwrap_or(0), i64)
                );
            }
            let start_root = vote_state.root_slot;

            // Add the last vote to update the `heaviest_subtree_fork_choice`
            if let Some(last_voted_slot) = vote_state.last_voted_slot() {
                pubkey_votes.push((key, last_voted_slot));
            }

            vote_state.process_slot_vote_unchecked(bank_slot);

            for vote in &vote_state.votes {
                bank_weight += vote.lockout() as u128 * voted_stake as u128;
                Self::populate_ancestor_voted_stakes(&mut voted_stakes, &vote, ancestors);
            }

            if start_root != vote_state.root_slot {
                if let Some(root) = start_root {
                    let vote = Lockout {
                        confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                        slot: root,
                    };
                    trace!("ROOT: {}", vote.slot);
                    bank_weight += vote.lockout() as u128 * voted_stake as u128;
                    Self::populate_ancestor_voted_stakes(&mut voted_stakes, &vote, ancestors);
                }
            }
            if let Some(root) = vote_state.root_slot {
                let vote = Lockout {
                    confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                    slot: root,
                };
                bank_weight += vote.lockout() as u128 * voted_stake as u128;
                Self::populate_ancestor_voted_stakes(&mut voted_stakes, &vote, ancestors);
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
                vote_state.nth_recent_vote(0).map(|l| l.slot),
                Some(bank_slot)
            );
            if let Some(vote) = vote_state.nth_recent_vote(1) {
                // Update all the parents of this last vote with the stake of this vote account
                Self::update_ancestor_voted_stakes(
                    &mut voted_stakes,
                    vote.slot,
                    voted_stake,
                    ancestors,
                );
            }
            total_stake += voted_stake;
        }

        ComputedBankState {
            voted_stakes,
            total_stake,
            bank_weight,
            lockout_intervals,
            pubkey_votes: Arc::new(pubkey_votes),
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

    fn new_vote(
        local_vote_state: &VoteState,
        slot: Slot,
        hash: Hash,
        last_voted_slot_in_bank: Option<Slot>,
    ) -> (Vote, usize) {
        let mut local_vote_state = local_vote_state.clone();
        let vote = Vote::new(vec![slot], hash);
        local_vote_state.process_vote_unchecked(&vote);
        let slots = if let Some(last_voted_slot_in_bank) = last_voted_slot_in_bank {
            local_vote_state
                .votes
                .iter()
                .map(|v| v.slot)
                .skip_while(|s| *s <= last_voted_slot_in_bank)
                .collect()
        } else {
            local_vote_state.votes.iter().map(|v| v.slot).collect()
        };
        trace!(
            "new vote with {:?} {:?} {:?}",
            last_voted_slot_in_bank,
            slots,
            local_vote_state.votes
        );
        (Vote::new(slots, hash), local_vote_state.votes.len() - 1)
    }

    fn last_voted_slot_in_bank(bank: &Bank, vote_account_pubkey: &Pubkey) -> Option<Slot> {
        let (_stake, vote_account) = bank.get_vote_account(vote_account_pubkey)?;
        let slot = vote_account.vote_state().as_ref().ok()?.last_voted_slot();
        slot
    }

    pub fn new_vote_from_bank(&self, bank: &Bank, vote_account_pubkey: &Pubkey) -> (Vote, usize) {
        let voted_slot = Self::last_voted_slot_in_bank(bank, vote_account_pubkey);
        Self::new_vote(&self.lockouts, bank.slot(), bank.hash(), voted_slot)
    }

    pub fn record_bank_vote(&mut self, vote: Vote) -> Option<Slot> {
        let slot = vote.last_voted_slot().unwrap_or(0);
        trace!("{} record_vote for {}", self.node_pubkey, slot);
        let old_root = self.root();
        self.lockouts.process_vote_unchecked(&vote);
        self.last_vote = vote;
        let new_root = self.root();

        datapoint_info!("tower-vote", ("latest", slot, i64), ("root", new_root, i64));
        if old_root != new_root {
            Some(new_root)
        } else {
            None
        }
    }

    #[cfg(test)]
    pub fn record_vote(&mut self, slot: Slot, hash: Hash) -> Option<Slot> {
        let vote = Vote::new(vec![slot], hash);
        self.record_bank_vote(vote)
    }

    pub fn last_voted_slot(&self) -> Option<Slot> {
        self.last_vote.last_voted_slot()
    }

    pub fn stray_restored_slot(&self) -> Option<Slot> {
        self.stray_restored_slot
    }

    pub fn last_vote_and_timestamp(&mut self) -> Vote {
        let mut last_vote = self.last_vote.clone();
        last_vote.timestamp = self.maybe_timestamp(last_vote.last_voted_slot().unwrap_or(0));
        last_vote
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
        self.lockouts.root_slot.unwrap()
    }

    // a slot is recent if it's newer than the last vote we have
    pub fn is_recent(&self, slot: Slot) -> bool {
        if let Some(last_voted_slot) = self.lockouts.last_voted_slot() {
            if slot <= last_voted_slot {
                return false;
            }
        }
        true
    }

    pub fn has_voted(&self, slot: Slot) -> bool {
        for vote in &self.lockouts.votes {
            if slot == vote.slot {
                return true;
            }
        }
        false
    }

    pub fn is_locked_out(&self, slot: Slot, ancestors: &HashMap<Slot, HashSet<Slot>>) -> bool {
        assert!(ancestors.contains_key(&slot));

        if !self.is_recent(slot) {
            return true;
        }

        let mut lockouts = self.lockouts.clone();
        lockouts.process_slot_vote_unchecked(slot);
        for vote in &lockouts.votes {
            if vote.slot == slot {
                continue;
            }
            if !ancestors[&slot].contains(&vote.slot) {
                return true;
            }
        }
        if let Some(root_slot) = lockouts.root_slot {
            // This case should never happen because bank forks purges all
            // non-descendants of the root every time root is set
            if slot != root_slot {
                assert!(
                    ancestors[&slot].contains(&root_slot),
                    "ancestors: {:?}, slot: {} root: {}",
                    ancestors[&slot],
                    slot,
                    root_slot
                );
            }
        }

        false
    }

    fn make_check_switch_threshold_decision(
        &self,
        switch_slot: u64,
        ancestors: &HashMap<Slot, HashSet<u64>>,
        descendants: &HashMap<Slot, HashSet<u64>>,
        progress: &ProgressMap,
        total_stake: u64,
        epoch_vote_accounts: &HashMap<Pubkey, (u64, ArcVoteAccount)>,
    ) -> SwitchForkDecision {
        self.last_voted_slot()
            .map(|last_voted_slot| {
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
                           last vote({}), meaning some inconsistency between saved tower and ledger.",
                          last_voted_slot
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

                let last_vote_ancestors =
                    ancestors.get(&last_voted_slot).unwrap_or_else(|| {
                        if !self.is_stray_last_vote() {
                            // Unless last vote is stray and stale, ancestors.get(last_voted_slot) must
                            // return Some(_), justifying to panic! here.
                            // Also, adjust_lockouts_after_replay() correctly makes last_voted_slot None,
                            // if all saved votes are ancestors of replayed_root_slot. So this code shouldn't be
                            // touched in that case as well.
                            // In other words, except being stray, all other slots have been voted on while
                            // this validator has been running, so we must be able to fetch ancestors for
                            // all of them.
                            panic!("no ancestors found with slot: {}", last_voted_slot);
                        } else {
                            empty_ancestors_due_to_minor_unsynced_ledger()
                        }
                    });

                let switch_slot_ancestors = ancestors.get(&switch_slot).unwrap();

                if switch_slot == last_voted_slot || switch_slot_ancestors.contains(&last_voted_slot) {
                    // If the `switch_slot is a descendant of the last vote,
                    // no switching proof is necessary
                    return SwitchForkDecision::SameFork;
                }

                if last_vote_ancestors.contains(&switch_slot) {
                    if !self.is_stray_last_vote() {
                        panic!(
                            "Should never consider switching to slot ({}), which is ancestors({:?}) of last vote: {}",
                            switch_slot,
                            last_vote_ancestors,
                            last_voted_slot
                        );
                    } else {
                        return suspended_decision_due_to_major_unsynced_ledger();
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
                    if !progress.get_fork_stats(*candidate_slot).map(|stats| stats.computed).unwrap_or(false)
                        // If any of the descendants have the `computed` flag set, then there must be a more
                        // recent frozen bank on this fork to use, so we can ignore this one. Otherwise,
                        // even if this bank has descendants, if they have not yet been frozen / stats computed,
                        // then use this bank as a representative for the fork.
                        || descendants.iter().any(|d| progress.get_fork_stats(*d).map(|stats| stats.computed).unwrap_or(false))
                        || *candidate_slot == last_voted_slot
                        || ancestors
                            .get(&candidate_slot)
                            .expect(
                                "empty descendants implies this is a child, not parent of root, so must
                                exist in the ancestors map",
                            )
                            .contains(&last_voted_slot)
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
                        .get(&candidate_slot)
                        .unwrap()
                        .fork_stats
                        .lockout_intervals;
                    // Find any locked out intervals in this bank with endpoint >= last_vote,
                    // implies they are locked out at last_vote
                    for (_lockout_interval_end, intervals_keyed_by_end) in lockout_intervals.range((Included(last_voted_slot), Unbounded)) {
                        for (lockout_interval_start, vote_account_pubkey) in intervals_keyed_by_end {
                            if locked_out_vote_accounts.contains(vote_account_pubkey) {
                                continue;
                            }

                            // Only count lockouts on slots that are:
                            // 1) Not ancestors of `last_vote`, meaning being on different fork
                            // 2) Not from before the current root as we can't determine if
                            // anything before the root was an ancestor of `last_vote` or not
                            if !last_vote_ancestors.contains(lockout_interval_start)
                                // Given a `lockout_interval_start` < root that appears in a
                                // bank for a `candidate_slot`, it must be that `lockout_interval_start`
                                // is an ancestor of the current root, because `candidate_slot` is a
                                // descendant of the current root
                                && *lockout_interval_start > root
                            {
                                let stake = epoch_vote_accounts
                                    .get(vote_account_pubkey)
                                    .map(|(stake, _)| *stake)
                                    .unwrap_or(0);
                                locked_out_stake += stake;
                                locked_out_vote_accounts.insert(vote_account_pubkey);
                            }
                        }
                    }
                }

                if (locked_out_stake as f64 / total_stake as f64) > SWITCH_FORK_THRESHOLD {
                    SwitchForkDecision::SwitchProof(switch_proof)
                } else {
                    SwitchForkDecision::FailedSwitchThreshold(locked_out_stake, total_stake)
                }
            })
            .unwrap_or(SwitchForkDecision::SameFork)
    }

    pub(crate) fn check_switch_threshold(
        &mut self,
        switch_slot: u64,
        ancestors: &HashMap<Slot, HashSet<u64>>,
        descendants: &HashMap<Slot, HashSet<u64>>,
        progress: &ProgressMap,
        total_stake: u64,
        epoch_vote_accounts: &HashMap<Pubkey, (u64, ArcVoteAccount)>,
    ) -> SwitchForkDecision {
        let decision = self.make_check_switch_threshold_decision(
            switch_slot,
            ancestors,
            descendants,
            progress,
            total_stake,
            epoch_vote_accounts,
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

    pub fn check_vote_stake_threshold(
        &self,
        slot: Slot,
        voted_stakes: &VotedStakes,
        total_stake: Stake,
    ) -> bool {
        let mut lockouts = self.lockouts.clone();
        lockouts.process_slot_vote_unchecked(slot);
        let vote = lockouts.nth_recent_vote(self.threshold_depth);
        if let Some(vote) = vote {
            if let Some(fork_stake) = voted_stakes.get(&vote.slot) {
                let lockout = *fork_stake as f64 / total_stake as f64;
                trace!(
                    "fork_stake slot: {}, vote slot: {}, lockout: {} fork_stake: {} total_stake: {}",
                    slot, vote.slot, lockout, fork_stake, total_stake
                );
                if vote.confirmation_count as usize > self.threshold_depth {
                    for old_vote in &self.lockouts.votes {
                        if old_vote.slot == vote.slot
                            && old_vote.confirmation_count == vote.confirmation_count
                        {
                            return true;
                        }
                    }
                }
                lockout > self.threshold_size
            } else {
                false
            }
        } else {
            true
        }
    }

    /// Update lockouts for all the ancestors
    pub(crate) fn populate_ancestor_voted_stakes(
        voted_stakes: &mut VotedStakes,
        vote: &Lockout,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
    ) {
        // If there's no ancestors, that means this slot must be from before the current root,
        // in which case the lockouts won't be calculated in bank_weight anyways, so ignore
        // this slot
        let vote_slot_ancestors = ancestors.get(&vote.slot);
        if vote_slot_ancestors.is_none() {
            return;
        }
        let mut slot_with_ancestors = vec![vote.slot];
        slot_with_ancestors.extend(vote_slot_ancestors.unwrap());
        for slot in slot_with_ancestors {
            voted_stakes.entry(slot).or_default();
        }
    }

    pub(crate) fn find_heaviest_bank(
        bank_forks: &BankForks,
        node_pubkey: &Pubkey,
    ) -> Option<Arc<Bank>> {
        let ancestors = bank_forks.ancestors();
        let mut bank_weights: Vec<_> = bank_forks
            .frozen_banks()
            .values()
            .map(|b| {
                (
                    Self::bank_weight(node_pubkey, b, &ancestors),
                    b.parents().len(),
                    b.clone(),
                )
            })
            .collect();
        bank_weights.sort_by_key(|b| (b.0, b.1));
        bank_weights.pop().map(|b| b.2)
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
        let vote_slot_ancestors = ancestors.get(&voted_slot);
        if vote_slot_ancestors.is_none() {
            return;
        }
        let mut slot_with_ancestors = vec![voted_slot];
        slot_with_ancestors.extend(vote_slot_ancestors.unwrap());
        for slot in slot_with_ancestors {
            let current = voted_stakes.entry(slot).or_default();
            *current += voted_stake;
        }
    }

    fn bank_weight(
        node_pubkey: &Pubkey,
        bank: &Bank,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
    ) -> u128 {
        let ComputedBankState { bank_weight, .. } = Self::collect_vote_lockouts(
            node_pubkey,
            bank.slot(),
            bank.vote_accounts().into_iter(),
            ancestors,
            &mut PubkeyReferences::default(),
        );
        bank_weight
    }

    fn voted_slots(&self) -> Vec<Slot> {
        self.lockouts
            .votes
            .iter()
            .map(|lockout| lockout.slot)
            .collect()
    }

    pub fn is_stray_last_vote(&self) -> bool {
        if let Some(last_voted_slot) = self.last_voted_slot() {
            if let Some(stray_restored_slot) = self.stray_restored_slot {
                return stray_restored_slot == last_voted_slot;
            }
        }

        false
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
            self.last_vote == Vote::default() && self.lockouts.votes.is_empty()
                || self.last_vote != Vote::default() && !self.lockouts.votes.is_empty(),
            format!(
                "last vote: {:?} lockouts.votes: {:?}",
                self.last_vote, self.lockouts.votes
            )
        );

        if let Some(last_voted_slot) = self.last_voted_slot() {
            if tower_root <= replayed_root {
                // Normally, we goes into this clause with possible help of
                // reconcile_blockstore_roots_with_tower()
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
                     VOTING will be SUSPENDED UNTIL {}!",
                    last_voted_slot,
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
            Vec::with_capacity(self.lockouts.votes.len());

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

        let original_votes_len = self.lockouts.votes.len();
        self.initialize_lockouts(move |_| retain_flags_for_each_vote.next().unwrap());

        if self.lockouts.votes.is_empty() {
            info!("All restored votes were behind; resetting root_slot and last_vote in tower!");
            // we might not have banks for those votes so just reset.
            // That's because the votes may well past replayed_root
            self.last_vote = Vote::default();
        } else {
            info!(
                "{} restored votes (out of {}) were on different fork or are upcoming votes on unrooted slots: {:?}!",
                self.voted_slots().len(),
                original_votes_len,
                self.voted_slots()
            );

            assert_eq!(
                self.last_vote.last_voted_slot().unwrap(),
                *self.voted_slots().last().unwrap()
            );
            self.stray_restored_slot = Some(self.last_vote.last_voted_slot().unwrap());
        }

        Ok(())
    }

    fn initialize_lockouts_from_bank(
        &mut self,
        vote_account_pubkey: &Pubkey,
        root: Slot,
        bank: &Bank,
    ) {
        if let Some((_stake, vote_account)) = bank.get_vote_account(vote_account_pubkey) {
            self.lockouts = vote_account
                .vote_state()
                .as_ref()
                .expect("vote_account isn't a VoteState?")
                .clone();
            self.initialize_root(root);
            self.initialize_lockouts(|v| v.slot > root);
            trace!(
                "Lockouts in tower for {} is initialized using bank {}",
                self.node_pubkey,
                bank.slot(),
            );
            assert_eq!(
                self.lockouts.node_pubkey, self.node_pubkey,
                "vote account's node_pubkey doesn't match",
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

    fn initialize_lockouts<F: FnMut(&Lockout) -> bool>(&mut self, should_retain: F) {
        self.lockouts.votes.retain(should_retain);
    }

    // Updating root is needed to correctly restore from newly-saved tower for the next
    // boot
    fn initialize_root(&mut self, root: Slot) {
        self.lockouts.root_slot = Some(root);
    }

    pub fn get_filename(path: &Path, node_pubkey: &Pubkey) -> PathBuf {
        path.join(format!("tower-{}", node_pubkey))
            .with_extension("bin")
    }

    pub fn get_tmp_filename(path: &Path) -> PathBuf {
        path.with_extension("bin.new")
    }

    pub fn save(&self, node_keypair: &Arc<Keypair>) -> Result<()> {
        let mut measure = Measure::start("tower_save-ms");

        if self.node_pubkey != node_keypair.pubkey() {
            return Err(TowerError::WrongTower(format!(
                "node_pubkey is {:?} but found tower for {:?}",
                node_keypair.pubkey(),
                self.node_pubkey
            )));
        }

        let filename = &self.path;
        let new_filename = &self.tmp_path;
        {
            // overwrite anything if exists
            let mut file = File::create(&new_filename)?;
            let saved_tower = SavedTower::new(self, node_keypair)?;
            bincode::serialize_into(&mut file, &saved_tower)?;
            // file.sync_all() hurts performance; pipeline sync-ing and submitting votes to the cluster!
        }
        trace!("persisted votes: {:?}", self.voted_slots());
        fs::rename(&new_filename, &filename)?;
        // self.path.parent().sync_all() hurts performance same as the above sync

        measure.stop();
        inc_new_counter_info!("tower_save-ms", measure.as_ms() as usize);

        Ok(())
    }

    pub fn restore(path: &Path, node_pubkey: &Pubkey) -> Result<Self> {
        let filename = Self::get_filename(path, node_pubkey);

        // Ensure to create parent dir here, because restore() precedes save() always
        fs::create_dir_all(&filename.parent().unwrap())?;

        let file = File::open(&filename)?;
        let mut stream = BufReader::new(file);

        let saved_tower: SavedTower = bincode::deserialize_from(&mut stream)?;
        if !saved_tower.verify(node_pubkey) {
            return Err(TowerError::InvalidSignature);
        }
        let mut tower = saved_tower.deserialize()?;
        tower.path = filename;
        tower.tmp_path = Self::get_tmp_filename(&tower.path);

        // check that the tower actually belongs to this node
        if &tower.node_pubkey != node_pubkey {
            return Err(TowerError::WrongTower(format!(
                "node_pubkey is {:?} but found tower for {:?}",
                node_pubkey, tower.node_pubkey
            )));
        }
        Ok(tower)
    }
}

#[derive(Error, Debug)]
pub enum TowerError {
    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),

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

impl TowerError {
    pub fn is_file_missing(&self) -> bool {
        if let TowerError::IOError(io_err) = &self {
            io_err.kind() == std::io::ErrorKind::NotFound
        } else {
            false
        }
    }
}

#[frozen_abi(digest = "Gaxfwvx5MArn52mKZQgzHmDCyn5YfCuTHvp5Et3rFfpp")]
#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, AbiExample)]
pub struct SavedTower {
    signature: Signature,
    data: Vec<u8>,
}

impl SavedTower {
    pub fn new<T: Signer>(tower: &Tower, keypair: &Arc<T>) -> Result<Self> {
        let data = bincode::serialize(tower)?;
        let signature = keypair.sign_message(&data);
        Ok(Self { data, signature })
    }

    pub fn verify(&self, pubkey: &Pubkey) -> bool {
        self.signature.verify(pubkey.as_ref(), &self.data)
    }

    pub fn deserialize(&self) -> Result<Tower> {
        bincode::deserialize(&self.data).map_err(|e| e.into())
    }
}

// Given an untimely crash, tower may have roots that are not reflected in blockstore,
// or the reverse of this.
// That's because we don't impose any ordering guarantee or any kind of write barriers
// between tower (plain old POSIX fs calls) and blockstore (through RocksDB), when
// `ReplayState::handle_votable_bank()` saves tower before setting blockstore roots.
pub fn reconcile_blockstore_roots_with_tower(
    tower: &Tower,
    blockstore: &Blockstore,
) -> blockstore_db::Result<()> {
    let tower_root = tower.root();
    let last_blockstore_root = blockstore.last_root();
    if last_blockstore_root < tower_root {
        // Ensure tower_root itself to exist and be marked as rooted in the blockstore
        // in addition to its ancestors.
        let new_roots: Vec<_> = AncestorIterator::new_inclusive(tower_root, &blockstore)
            .take_while(|current| match current.cmp(&last_blockstore_root) {
                Ordering::Greater => true,
                Ordering::Equal => false,
                Ordering::Less => panic!(
                    "couldn't find a last_blockstore_root upwards from: {}!?",
                    tower_root
                ),
            })
            .collect();
        if !new_roots.is_empty() {
            info!(
                "Reconciling slots as root based on tower root: {:?} ({}..{}) ",
                new_roots, tower_root, last_blockstore_root
            );
            blockstore.set_roots(&new_roots)?;
        } else {
            // This indicates we're in bad state; but still don't panic here.
            // That's because we might have a chance of recovering properly with
            // newer snapshot.
            warn!(
                "Couldn't find any ancestor slots from tower root ({}) \
                 towards blockstore root ({}); blockstore pruned or only \
                 tower moved into new ledger?",
                tower_root, last_blockstore_root,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::{
        bank_weight_fork_choice::BankWeightForkChoice,
        cluster_info_vote_listener::VoteTracker,
        cluster_slots::ClusterSlots,
        fork_choice::SelectVoteAndResetForkResult,
        heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
        progress_map::ForkProgress,
        replay_stage::{HeaviestForkFailures, ReplayStage},
    };
    use solana_ledger::{blockstore::make_slot_entries, get_tmp_ledger_path};
    use solana_runtime::{
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{
            create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
        },
    };
    use solana_sdk::{
        account::Account, clock::Slot, hash::Hash, pubkey::Pubkey, signature::Signer,
        slot_history::SlotHistory,
    };
    use solana_vote_program::{
        vote_state::{Vote, VoteStateVersions, MAX_LOCKOUT_HISTORY},
        vote_transaction,
    };
    use std::{
        collections::HashMap,
        fs::{remove_file, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        rc::Rc,
        sync::RwLock,
    };
    use tempfile::TempDir;
    use trees::{tr, Tree, TreeWalk};

    pub(crate) struct VoteSimulator {
        pub validator_keypairs: HashMap<Pubkey, ValidatorVoteKeypairs>,
        pub node_pubkeys: Vec<Pubkey>,
        pub vote_pubkeys: Vec<Pubkey>,
        pub bank_forks: RwLock<BankForks>,
        pub progress: ProgressMap,
        pub heaviest_subtree_fork_choice: HeaviestSubtreeForkChoice,
    }

    impl VoteSimulator {
        pub(crate) fn new(num_keypairs: usize) -> Self {
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
                bank_forks: RwLock::new(bank_forks),
                progress,
                heaviest_subtree_fork_choice,
            }
        }
        pub(crate) fn fill_bank_forks(
            &mut self,
            forks: Tree<u64>,
            cluster_votes: &HashMap<Pubkey, Vec<u64>>,
        ) {
            let root = forks.root().data;
            assert!(self.bank_forks.read().unwrap().get(root).is_some());

            let mut walk = TreeWalk::from(forks);

            while let Some(visit) = walk.get() {
                let slot = visit.node().data;
                self.progress
                    .entry(slot)
                    .or_insert_with(|| ForkProgress::new(Hash::default(), None, None, 0, 0));
                if self.bank_forks.read().unwrap().get(slot).is_some() {
                    walk.forward();
                    continue;
                }
                let parent = walk.get_parent().unwrap().data;
                let parent_bank = self.bank_forks.read().unwrap().get(parent).unwrap().clone();
                let new_bank = Bank::new_from_parent(&parent_bank, &Pubkey::default(), slot);
                for (pubkey, vote) in cluster_votes.iter() {
                    if vote.contains(&parent) {
                        let keypairs = self.validator_keypairs.get(pubkey).unwrap();
                        let last_blockhash = parent_bank.last_blockhash();
                        let vote_tx = vote_transaction::new_vote_transaction(
                            // Must vote > root to be processed
                            vec![parent],
                            parent_bank.hash(),
                            last_blockhash,
                            &keypairs.node_keypair,
                            &keypairs.vote_keypair,
                            &keypairs.vote_keypair,
                            None,
                        );
                        info!("voting {} {}", parent_bank.slot(), parent_bank.hash());
                        new_bank.process_transaction(&vote_tx).unwrap();
                    }
                }
                new_bank.freeze();
                self.heaviest_subtree_fork_choice
                    .add_new_leaf_slot(new_bank.slot(), Some(new_bank.parent_slot()));
                self.bank_forks.write().unwrap().insert(new_bank);
                walk.forward();
            }
        }

        pub(crate) fn simulate_vote(
            &mut self,
            vote_slot: Slot,
            my_pubkey: &Pubkey,
            tower: &mut Tower,
        ) -> Vec<HeaviestForkFailures> {
            // Try to simulate the vote
            let my_keypairs = self.validator_keypairs.get(&my_pubkey).unwrap();
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
                &my_pubkey,
                &ancestors,
                &mut frozen_banks,
                tower,
                &mut self.progress,
                &VoteTracker::default(),
                &ClusterSlots::default(),
                &self.bank_forks,
                &mut PubkeyReferences::default(),
                &mut self.heaviest_subtree_fork_choice,
                &mut BankWeightForkChoice::default(),
            );

            let vote_bank = self
                .bank_forks
                .read()
                .unwrap()
                .get(vote_slot)
                .expect("Bank must have been created before vote simulation")
                .clone();

            // Try to vote on the given slot
            let descendants = self.bank_forks.read().unwrap().descendants();
            let SelectVoteAndResetForkResult {
                heaviest_fork_failures,
                ..
            } = ReplayStage::select_vote_and_reset_forks(
                &vote_bank,
                &None,
                &ancestors,
                &descendants,
                &self.progress,
                tower,
            );

            // Make sure this slot isn't locked out or failing threshold
            info!("Checking vote: {}", vote_bank.slot());
            if !heaviest_fork_failures.is_empty() {
                return heaviest_fork_failures;
            }
            let vote = tower.new_vote_from_bank(&vote_bank, &my_vote_pubkey).0;
            if let Some(new_root) = tower.record_bank_vote(vote) {
                self.set_root(new_root);
            }

            vec![]
        }

        pub fn set_root(&mut self, new_root: Slot) {
            ReplayStage::handle_new_root(
                new_root,
                &self.bank_forks,
                &mut self.progress,
                &None,
                &mut PubkeyReferences::default(),
                None,
                &mut self.heaviest_subtree_fork_choice,
            )
        }

        fn create_and_vote_new_branch(
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
                    self.fill_bank_forks(fork_tip_parent, &cluster_votes);
                    if votes_to_simulate.contains(&slot) {
                        Some((slot, self.simulate_vote(slot, &my_pubkey, tower)))
                    } else {
                        None
                    }
                })
                .collect()
        }

        fn simulate_lockout_interval(
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
                .push((lockout_interval.0, Rc::new(*vote_account_pubkey)));
        }

        fn can_progress_on_fork(
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
                self.fill_bank_forks(fork_tip_parent, cluster_votes);
                if self
                    .simulate_vote(i + start_slot, &my_pubkey, tower)
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
    pub(crate) fn initialize_state(
        validator_keypairs_map: &HashMap<Pubkey, ValidatorVoteKeypairs>,
        stake: u64,
    ) -> (BankForks, ProgressMap, HeaviestSubtreeForkChoice) {
        let validator_keypairs: Vec<_> = validator_keypairs_map.values().collect();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            voting_keypair: _,
        } = create_genesis_config_with_vote_accounts(
            1_000_000_000,
            &validator_keypairs,
            vec![stake; validator_keypairs.len()],
        );

        let bank0 = Bank::new(&genesis_config);

        for pubkey in validator_keypairs_map.keys() {
            bank0.transfer(10_000, &mint_keypair, pubkey).unwrap();
        }

        bank0.freeze();
        let mut progress = ProgressMap::default();
        progress.insert(
            0,
            ForkProgress::new(bank0.last_blockhash(), None, None, 0, 0),
        );
        let bank_forks = BankForks::new(bank0);
        let heaviest_subtree_fork_choice =
            HeaviestSubtreeForkChoice::new_from_bank_forks(&bank_forks);
        (bank_forks, progress, heaviest_subtree_fork_choice)
    }

    fn gen_stakes(stake_votes: &[(u64, &[u64])]) -> Vec<(Pubkey, (u64, ArcVoteAccount))> {
        let mut stakes = vec![];
        for (lamports, votes) in stake_votes {
            let mut account = Account::default();
            account.data = vec![0; VoteState::size_of()];
            account.lamports = *lamports;
            let mut vote_state = VoteState::default();
            for slot in *votes {
                vote_state.process_slot_vote_unchecked(*slot);
            }
            VoteState::serialize(
                &VoteStateVersions::Current(Box::new(vote_state)),
                &mut account.data,
            )
            .expect("serialize state");
            stakes.push((
                solana_sdk::pubkey::new_rand(),
                (*lamports, ArcVoteAccount::from(account)),
            ));
        }
        stakes
    }

    #[test]
    fn test_to_vote_instruction() {
        let vote = Vote::default();
        let mut decision = SwitchForkDecision::FailedSwitchThreshold(0, 1);
        assert!(decision
            .to_vote_instruction(vote.clone(), &Pubkey::default(), &Pubkey::default())
            .is_none());
        decision = SwitchForkDecision::SameFork;
        assert_eq!(
            decision.to_vote_instruction(vote.clone(), &Pubkey::default(), &Pubkey::default()),
            Some(vote_instruction::vote(
                &Pubkey::default(),
                &Pubkey::default(),
                vote.clone(),
            ))
        );
        decision = SwitchForkDecision::SwitchProof(Hash::default());
        assert_eq!(
            decision.to_vote_instruction(vote.clone(), &Pubkey::default(), &Pubkey::default()),
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
        let mut tower = Tower::new_with_key(&node_pubkey);

        // Create the tree of banks
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3) / (tr(4) / tr(5)))));

        // Set the voting behavior
        let mut cluster_votes = HashMap::new();
        let votes = vec![0, 1, 2, 3, 4, 5];
        cluster_votes.insert(node_pubkey, votes.clone());
        vote_simulator.fill_bank_forks(forks, &cluster_votes);

        // Simulate the votes
        for vote in votes {
            assert!(vote_simulator
                .simulate_vote(vote, &node_pubkey, &mut tower,)
                .is_empty());
        }

        for i in 0..5 {
            assert_eq!(tower.lockouts.votes[i].slot as usize, i);
            assert_eq!(tower.lockouts.votes[i].confirmation_count as usize, 6 - i);
        }
    }

    #[test]
    fn test_switch_threshold() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(2);
        let my_pubkey = vote_simulator.node_pubkeys[0];
        let other_vote_account = vote_simulator.vote_pubkeys[1];
        let bank0 = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(0)
            .unwrap()
            .clone();
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
                            / (tr(110))))));

        // Fill the BankForks according to the above fork structure
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
        for (_, fork_progress) in vote_simulator.progress.iter_mut() {
            fork_progress.fork_stats.computed = true;
        }
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let mut descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        let mut tower = Tower::new_with_key(&my_pubkey);

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
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        // If we set a root, then any lockout intervals below the root shouldn't
        // count toward the switch threshold. This means the other validator's
        // vote lockout no longer counts
        tower.lockouts.root_slot = Some(43);
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
            ),
            SwitchForkDecision::FailedSwitchThreshold(0, 20000)
        );
    }

    #[test]
    fn test_switch_threshold_votes() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(4);
        let my_pubkey = vote_simulator.node_pubkeys[0];
        let mut tower = Tower::new_with_key(&my_pubkey);
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
        vote_simulator.fill_bank_forks(forks, &cluster_votes);

        // Vote on the first minor fork at slot 14, should succeed
        assert!(vote_simulator
            .simulate_vote(14, &my_pubkey, &mut tower,)
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
            &my_pubkey,
            &mut tower,
        );
        for slot in 46..=48 {
            if slot == 48 {
                assert!(results.get(&slot).unwrap().is_empty());
            } else {
                assert_eq!(
                    *results.get(&slot).unwrap(),
                    vec![HeaviestForkFailures::FailedSwitchThreshold(slot)]
                );
            }
        }
    }

    #[test]
    fn test_double_partition() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(2);
        let node_pubkey = vote_simulator.node_pubkeys[0];
        let vote_pubkey = vote_simulator.vote_pubkeys[0];
        let mut tower = Tower::new_with_key(&node_pubkey);

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
        my_votes.extend(0..=14);
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
        vote_simulator.fill_bank_forks(forks, &cluster_votes);

        // Simulate the votes.
        for vote in &my_votes {
            // All these votes should be ok
            assert!(vote_simulator
                .simulate_vote(*vote, &node_pubkey, &mut tower,)
                .is_empty());
        }

        info!("local tower: {:#?}", tower.lockouts.votes);
        let observed = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(next_unlocked_slot)
            .unwrap()
            .get_vote_account(&vote_pubkey)
            .unwrap();
        let state = observed.1.vote_state();
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
        let mut accounts = gen_stakes(&[(1, &[0]), (1, &[0])]);
        accounts.sort_by_key(|(pk, _)| *pk);
        let account_latest_votes: PubkeyVotes =
            accounts.iter().map(|(pubkey, _)| (*pubkey, 0)).collect();

        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        let ComputedBankState {
            voted_stakes,
            total_stake,
            bank_weight,
            pubkey_votes,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            1,
            accounts.into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        assert_eq!(voted_stakes[&0], 2);
        assert_eq!(total_stake, 2);
        let mut pubkey_votes = Arc::try_unwrap(pubkey_votes).unwrap();
        pubkey_votes.sort();
        assert_eq!(pubkey_votes, account_latest_votes);

        // Each account has 1 vote in it. After simulating a vote in collect_vote_lockouts,
        // the account will have 2 votes, with lockout 2 + 4 = 6. So expected weight for
        assert_eq!(bank_weight, 12)
    }

    #[test]
    fn test_collect_vote_lockouts_root() {
        let votes: Vec<u64> = (0..MAX_LOCKOUT_HISTORY as u64).collect();
        //two accounts voting for slots 0..MAX_LOCKOUT_HISTORY with 1 token staked
        let mut accounts = gen_stakes(&[(1, &votes), (1, &votes)]);
        accounts.sort_by_key(|(pk, _)| *pk);
        let account_latest_votes: PubkeyVotes = accounts
            .iter()
            .map(|(pubkey, _)| (*pubkey, (MAX_LOCKOUT_HISTORY - 1) as Slot))
            .collect();
        let mut tower = Tower::new_for_tests(0, 0.67);
        let mut ancestors = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            tower.record_vote(i as u64, Hash::default());
            ancestors.insert(i as u64, (0..i as u64).collect());
        }
        let root = Lockout {
            confirmation_count: MAX_LOCKOUT_HISTORY as u32,
            slot: 0,
        };
        let root_weight = root.lockout() as u128;
        let vote_account_expected_weight = tower
            .lockouts
            .votes
            .iter()
            .map(|v| v.lockout() as u128)
            .sum::<u128>()
            + root_weight;
        let expected_bank_weight = 2 * vote_account_expected_weight;
        assert_eq!(tower.lockouts.root_slot, Some(0));
        let ComputedBankState {
            voted_stakes,
            bank_weight,
            pubkey_votes,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            MAX_LOCKOUT_HISTORY as u64,
            accounts.into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        for i in 0..MAX_LOCKOUT_HISTORY {
            assert_eq!(voted_stakes[&(i as u64)], 2);
        }

        // should be the sum of all the weights for root
        assert_eq!(bank_weight, expected_bank_weight);
        let mut pubkey_votes = Arc::try_unwrap(pubkey_votes).unwrap();
        pubkey_votes.sort();
        assert_eq!(pubkey_votes, account_latest_votes);
    }

    #[test]
    fn test_check_vote_threshold_without_votes() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1 as Stake)].into_iter().collect();
        assert!(tower.check_vote_stake_threshold(0, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_no_skip_lockout_with_new_root() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(4, 0.67);
        let mut stakes = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY as u64 + 1) {
            stakes.insert(i, 1 as Stake);
            tower.record_vote(i, Hash::default());
        }
        assert!(!tower.check_vote_stake_threshold(MAX_LOCKOUT_HISTORY as u64 + 1, &stakes, 2,));
    }

    #[test]
    fn test_is_slot_confirmed_not_enough_stake_failure() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1 as Stake)].into_iter().collect();
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
        let stakes = vec![(0, 2 as Stake)].into_iter().collect();
        assert!(tower.is_slot_confirmed(0, &stakes, 2));
    }

    #[test]
    fn test_is_locked_out_empty() {
        let tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(0, HashSet::new())].into_iter().collect();
        assert!(!tower.is_locked_out(0, &ancestors));
    }

    #[test]
    fn test_is_locked_out_root_slot_child_pass() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect())]
            .into_iter()
            .collect();
        tower.lockouts.root_slot = Some(0);
        assert!(!tower.is_locked_out(1, &ancestors));
    }

    #[test]
    fn test_is_locked_out_root_slot_sibling_fail() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(2, vec![0].into_iter().collect())]
            .into_iter()
            .collect();
        tower.lockouts.root_slot = Some(0);
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
        assert!(tower.is_recent(0));
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
        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(0, &ancestors));
    }

    #[test]
    fn test_is_locked_out_child() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect())]
            .into_iter()
            .collect();
        tower.record_vote(0, Hash::default());
        assert!(!tower.is_locked_out(1, &ancestors));
    }

    #[test]
    fn test_is_locked_out_sibling() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![
            (0, HashSet::new()),
            (1, vec![0].into_iter().collect()),
            (2, vec![0].into_iter().collect()),
        ]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(2, &ancestors));
    }

    #[test]
    fn test_is_locked_out_last_vote_expired() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![
            (0, HashSet::new()),
            (1, vec![0].into_iter().collect()),
            (4, vec![0].into_iter().collect()),
        ]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(!tower.is_locked_out(4, &ancestors));
        tower.record_vote(4, Hash::default());
        assert_eq!(tower.lockouts.votes[0].slot, 0);
        assert_eq!(tower.lockouts.votes[0].confirmation_count, 2);
        assert_eq!(tower.lockouts.votes[1].slot, 4);
        assert_eq!(tower.lockouts.votes[1].confirmation_count, 1);
    }

    #[test]
    fn test_check_vote_threshold_below_threshold() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1 as Stake)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        assert!(!tower.check_vote_stake_threshold(1, &stakes, 2));
    }
    #[test]
    fn test_check_vote_threshold_above_threshold() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 2 as Stake)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        assert!(tower.check_vote_stake_threshold(1, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_after_pop() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 2 as Stake)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        assert!(tower.check_vote_stake_threshold(6, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_no_stake() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = HashMap::new();
        tower.record_vote(0, Hash::default());
        assert!(!tower.check_vote_stake_threshold(1, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_lockouts_not_updated() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(0, 1 as Stake), (1, 2 as Stake)].into_iter().collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        assert!(tower.check_vote_stake_threshold(6, &stakes, 2,));
    }

    #[test]
    fn test_stake_is_updated_for_entire_branch() {
        let mut voted_stakes = HashMap::new();
        let mut account = Account::default();
        account.lamports = 1;
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let ancestors: HashMap<u64, HashSet<u64>> = [(2u64, set)].iter().cloned().collect();
        Tower::update_ancestor_voted_stakes(&mut voted_stakes, 2, account.lamports, &ancestors);
        assert_eq!(voted_stakes[&0], 1);
        assert_eq!(voted_stakes[&1], 1);
        assert_eq!(voted_stakes[&2], 1);
    }

    #[test]
    fn test_new_vote() {
        let local = VoteState::default();
        let vote = Tower::new_vote(&local, 0, Hash::default(), None);
        assert_eq!(local.votes.len(), 0);
        assert_eq!(vote.0.slots, vec![0]);
        assert_eq!(vote.1, 0);
    }

    #[test]
    fn test_new_vote_dup_vote() {
        let local = VoteState::default();
        let vote = Tower::new_vote(&local, 0, Hash::default(), Some(0));
        assert!(vote.0.slots.is_empty());
    }

    #[test]
    fn test_new_vote_next_vote() {
        let mut local = VoteState::default();
        let vote = Vote {
            slots: vec![0],
            hash: Hash::default(),
            timestamp: None,
        };
        local.process_vote_unchecked(&vote);
        assert_eq!(local.votes.len(), 1);
        let vote = Tower::new_vote(&local, 1, Hash::default(), Some(0));
        assert_eq!(vote.0.slots, vec![1]);
        assert_eq!(vote.1, 1);
    }

    #[test]
    fn test_new_vote_next_after_expired_vote() {
        let mut local = VoteState::default();
        let vote = Vote {
            slots: vec![0],
            hash: Hash::default(),
            timestamp: None,
        };
        local.process_vote_unchecked(&vote);
        assert_eq!(local.votes.len(), 1);
        let vote = Tower::new_vote(&local, 3, Hash::default(), Some(0));
        //first vote expired, so index should be 0
        assert_eq!(vote.0.slots, vec![3]);
        assert_eq!(vote.1, 0);
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
            accounts.clone().into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        assert!(tower.check_vote_stake_threshold(vote_to_evaluate, &voted_stakes, total_stake,));

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
            accounts.into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        assert!(!tower.check_vote_stake_threshold(vote_to_evaluate, &voted_stakes, total_stake,));
    }

    fn vote_and_check_recent(num_votes: usize) {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let slots = if num_votes > 0 {
            vec![num_votes as u64 - 1]
        } else {
            vec![]
        };
        let expected = Vote::new(slots, Hash::default());
        for i in 0..num_votes {
            tower.record_vote(i as u64, Hash::default());
        }
        assert_eq!(expected, tower.last_vote)
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

    fn run_test_load_tower_snapshot<F, G>(
        modify_original: F,
        modify_serialized: G,
    ) -> (Tower, Result<Tower>)
    where
        F: Fn(&mut Tower, &Pubkey),
        G: Fn(&PathBuf),
    {
        let dir = TempDir::new().unwrap();
        let identity_keypair = Arc::new(Keypair::new());

        // Use values that will not match the default derived from BankForks
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.path = Tower::get_filename(&dir.path().to_path_buf(), &identity_keypair.pubkey());
        tower.tmp_path = Tower::get_tmp_filename(&tower.path);

        modify_original(&mut tower, &identity_keypair.pubkey());

        tower.save(&identity_keypair).unwrap();
        modify_serialized(&tower.path);
        let loaded = Tower::restore(&dir.path(), &identity_keypair.pubkey());

        (tower, loaded)
    }

    #[test]
    fn test_switch_threshold_across_tower_reload() {
        solana_logger::setup();
        // Init state
        let mut vote_simulator = VoteSimulator::new(2);
        let my_pubkey = vote_simulator.node_pubkeys[0];
        let other_vote_account = vote_simulator.vote_pubkeys[1];
        let bank0 = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(0)
            .unwrap()
            .clone();
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
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
        for (_, fork_progress) in vote_simulator.progress.iter_mut() {
            fork_progress.fork_stats.computed = true;
        }

        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();
        let mut tower = Tower::new_with_key(&my_pubkey);

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
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        assert_eq!(tower.voted_slots(), vec![43, 44, 45, 46, 47, 48, 49]);
        {
            let mut tower = tower.clone();
            tower.record_vote(110, Hash::default());
            tower.record_vote(111, Hash::default());
            assert_eq!(tower.voted_slots(), vec![43, 110, 111]);
            assert_eq!(tower.lockouts.root_slot, Some(0));
        }

        // Prepare simulated validator restart!
        let mut vote_simulator = VoteSimulator::new(2);
        let other_vote_account = vote_simulator.vote_pubkeys[1];
        let bank0 = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(0)
            .unwrap()
            .clone();
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
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
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
            ),
            SwitchForkDecision::SwitchProof(Hash::default())
        );

        tower.record_vote(110, Hash::default());
        tower.record_vote(111, Hash::default());
        assert_eq!(tower.voted_slots(), vec![110, 111]);
        assert_eq!(tower.lockouts.root_slot, Some(replayed_root_slot));
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
        let tower = Tower::new_with_key(&Pubkey::default());
        assert_matches!(
            tower.save(&identity_keypair),
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
                let mut buf = [0u8];
                assert_eq!(file.read(&mut buf).unwrap(), 1);
                buf[0] = !buf[0];
                assert_eq!(file.seek(SeekFrom::Start(0)).unwrap(), 0);
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
                    .open(&path)
                    .unwrap_or_else(|_| panic!("Failed to truncate file: {:?}", path));
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
        assert_matches!(loaded, Err(TowerError::IOError(_)))
    }

    #[test]
    fn test_reconcile_blockstore_roots_with_tower_normal() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(1, 0, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(3, 1, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(4, 1, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            assert!(!blockstore.is_root(0));
            assert!(!blockstore.is_root(1));
            assert!(!blockstore.is_root(3));
            assert!(!blockstore.is_root(4));

            let mut tower = Tower::new_with_key(&Pubkey::default());
            tower.lockouts.root_slot = Some(4);
            reconcile_blockstore_roots_with_tower(&tower, &blockstore).unwrap();

            assert!(!blockstore.is_root(0));
            assert!(blockstore.is_root(1));
            assert!(!blockstore.is_root(3));
            assert!(blockstore.is_root(4));
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    #[should_panic(expected = "couldn't find a last_blockstore_root upwards from: 4!?")]
    fn test_reconcile_blockstore_roots_with_tower_panic_no_common_root() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(1, 0, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(3, 1, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(4, 1, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            blockstore.set_roots(&[3]).unwrap();
            assert!(!blockstore.is_root(0));
            assert!(!blockstore.is_root(1));
            assert!(blockstore.is_root(3));
            assert!(!blockstore.is_root(4));

            let mut tower = Tower::new_with_key(&Pubkey::default());
            tower.lockouts.root_slot = Some(4);
            reconcile_blockstore_roots_with_tower(&tower, &blockstore).unwrap();
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_reconcile_blockstore_roots_with_tower_nop_no_parent() {
        solana_logger::setup();
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(1, 0, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let (shreds, _) = make_slot_entries(3, 1, 42);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            assert!(!blockstore.is_root(0));
            assert!(!blockstore.is_root(1));
            assert!(!blockstore.is_root(3));

            let mut tower = Tower::new_with_key(&Pubkey::default());
            tower.lockouts.root_slot = Some(4);
            assert_eq!(blockstore.last_root(), 0);
            reconcile_blockstore_roots_with_tower(&tower, &blockstore).unwrap();
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
        tower.lockouts.root_slot = Some(4);
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
        tower.lockouts.root_slot = Some(2);
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
        tower.lockouts.votes.push_back(Lockout::new(1));
        tower.lockouts.votes.push_back(Lockout::new(0));
        let vote = Vote::new(vec![0], Hash::default());
        tower.last_vote = vote;

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
        tower.lockouts.votes.push_back(Lockout::new(1));
        tower.lockouts.votes.push_back(Lockout::new(2));
        let vote = Vote::new(vec![2], Hash::default());
        tower.last_vote = vote;

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
            .lockouts
            .votes
            .push_back(Lockout::new(MAX_ENTRIES - 1));
        tower.lockouts.votes.push_back(Lockout::new(0));
        tower.lockouts.votes.push_back(Lockout::new(1));
        let vote = Vote::new(vec![1], Hash::default());
        tower.last_vote = vote;

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
        tower.lockouts.votes.push_back(Lockout::new(2));
        tower.lockouts.votes.push_back(Lockout::new(1));
        let vote = Vote::new(vec![1], Hash::default());
        tower.last_vote = vote;

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
        tower.lockouts.votes.push_back(Lockout::new(2));
        tower.lockouts.votes.push_back(Lockout::new(3));
        tower.lockouts.votes.push_back(Lockout::new(3));
        let vote = Vote::new(vec![3], Hash::default());
        tower.last_vote = vote;

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
        tower.lockouts.root_slot = Some(42);
        tower.lockouts.votes.push_back(Lockout::new(42));
        tower.lockouts.votes.push_back(Lockout::new(43));
        tower.lockouts.votes.push_back(Lockout::new(44));
        let vote = Vote::new(vec![44], Hash::default());
        tower.last_vote = vote;

        let mut slot_history = SlotHistory::default();
        slot_history.add(42);

        let tower = tower.adjust_lockouts_after_replay(42, &slot_history);
        assert_eq!(tower.unwrap().voted_slots(), [43, 44]);
    }

    #[test]
    fn test_adjust_lockouts_after_replay_vote_on_genesis() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.lockouts.votes.push_back(Lockout::new(0));
        let vote = Vote::new(vec![0], Hash::default());
        tower.last_vote = vote;

        let mut slot_history = SlotHistory::default();
        slot_history.add(0);

        assert!(tower.adjust_lockouts_after_replay(0, &slot_history).is_ok());
    }

    #[test]
    fn test_adjust_lockouts_after_replay_future_tower() {
        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.lockouts.votes.push_back(Lockout::new(13));
        tower.lockouts.votes.push_back(Lockout::new(14));
        let vote = Vote::new(vec![14], Hash::default());
        tower.last_vote = vote;
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
}
