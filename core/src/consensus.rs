use crate::{
    progress_map::{LockoutIntervals, ProgressMap},
    pubkey_references::PubkeyReferences,
};
use chrono::prelude::*;
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    hash::Hash,
    instruction::Instruction,
    pubkey::Pubkey,
};
use solana_vote_program::{
    vote_instruction,
    vote_state::{
        BlockTimestamp, Lockout, Vote, VoteState, MAX_LOCKOUT_HISTORY, TIMESTAMP_SLOT_INTERVAL,
    },
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Bound::{Included, Unbounded},
    sync::{Arc, RwLock},
};

#[derive(PartialEq, Clone, Debug)]
pub enum SwitchForkDecision {
    SwitchProof(Hash),
    NoSwitch,
    FailedSwitchThreshold,
}

impl SwitchForkDecision {
    pub fn to_vote_instruction(
        &self,
        vote: Vote,
        vote_account_pubkey: &Pubkey,
        authorized_voter_pubkey: &Pubkey,
    ) -> Option<Instruction> {
        match self {
            SwitchForkDecision::FailedSwitchThreshold => None,
            SwitchForkDecision::NoSwitch => Some(vote_instruction::vote(
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
}

pub const VOTE_THRESHOLD_DEPTH: usize = 8;
pub const VOTE_THRESHOLD_SIZE: f64 = 2f64 / 3f64;
pub const SWITCH_FORK_THRESHOLD: f64 = 0.38;

#[derive(Default, Debug, Clone)]
pub struct StakeLockout {
    lockout: u64,
    stake: u64,
}

impl StakeLockout {
    pub fn new(lockout: u64, stake: u64) -> Self {
        Self { lockout, stake }
    }
    pub fn lockout(&self) -> u64 {
        self.lockout
    }
    pub fn stake(&self) -> u64 {
        self.stake
    }
}

pub(crate) struct ComputedBankState {
    pub stake_lockouts: HashMap<Slot, StakeLockout>,
    pub total_staked: u64,
    pub bank_weight: u128,
    pub lockout_intervals: LockoutIntervals,
    pub pubkey_votes: Vec<(Pubkey, Slot)>,
}

pub struct Tower {
    node_pubkey: Pubkey,
    threshold_depth: usize,
    threshold_size: f64,
    lockouts: VoteState,
    last_vote: Vote,
    last_timestamp: BlockTimestamp,
}

impl Default for Tower {
    fn default() -> Self {
        Self {
            node_pubkey: Pubkey::default(),
            threshold_depth: VOTE_THRESHOLD_DEPTH,
            threshold_size: VOTE_THRESHOLD_SIZE,
            lockouts: VoteState::default(),
            last_vote: Vote::default(),
            last_timestamp: BlockTimestamp::default(),
        }
    }
}

impl Tower {
    pub fn new(
        node_pubkey: &Pubkey,
        vote_account_pubkey: &Pubkey,
        root: Slot,
        heaviest_bank: &Bank,
    ) -> Self {
        let mut tower = Self::new_with_key(node_pubkey);
        tower.initialize_lockouts_from_bank_forks(vote_account_pubkey, root, heaviest_bank);

        tower
    }

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

    pub(crate) fn collect_vote_lockouts<F>(
        node_pubkey: &Pubkey,
        bank_slot: u64,
        vote_accounts: F,
        ancestors: &HashMap<Slot, HashSet<u64>>,
        all_pubkeys: &mut PubkeyReferences,
    ) -> ComputedBankState
    where
        F: Iterator<Item = (Pubkey, (u64, Account))>,
    {
        let mut stake_lockouts = HashMap::new();
        let mut total_staked = 0;
        let mut bank_weight = 0;
        // Tree of intervals of lockouts of the form [slot, slot + slot.lockout],
        // keyed by end of the range
        let mut lockout_intervals = BTreeMap::new();
        let mut pubkey_votes = vec![];
        for (key, (lamports, account)) in vote_accounts {
            if lamports == 0 {
                continue;
            }
            trace!("{} {} with stake {}", node_pubkey, key, lamports);
            let vote_state = VoteState::from(&account);
            if vote_state.is_none() {
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
            let mut vote_state = vote_state.unwrap();

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

            // Add the latest vote to update the `heaviest_subtree_fork_choice`
            if let Some(latest_vote) = vote_state.votes.back() {
                pubkey_votes.push((key, latest_vote.slot));
            }

            vote_state.process_slot_vote_unchecked(bank_slot);

            for vote in &vote_state.votes {
                bank_weight += vote.lockout() as u128 * lamports as u128;
                Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
            }

            if start_root != vote_state.root_slot {
                if let Some(root) = start_root {
                    let vote = Lockout {
                        confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                        slot: root,
                    };
                    trace!("ROOT: {}", vote.slot);
                    bank_weight += vote.lockout() as u128 * lamports as u128;
                    Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
                }
            }
            if let Some(root) = vote_state.root_slot {
                let vote = Lockout {
                    confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                    slot: root,
                };
                bank_weight += vote.lockout() as u128 * lamports as u128;
                Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
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
                Self::update_ancestor_stakes(&mut stake_lockouts, vote.slot, lamports, ancestors);
            }
            total_staked += lamports;
        }

        ComputedBankState {
            stake_lockouts,
            total_staked,
            bank_weight,
            lockout_intervals,
            pubkey_votes,
        }
    }

    pub fn is_slot_confirmed(
        &self,
        slot: u64,
        lockouts: &HashMap<u64, StakeLockout>,
        total_staked: u64,
    ) -> bool {
        lockouts
            .get(&slot)
            .map(|lockout| (lockout.stake as f64 / total_staked as f64) > self.threshold_size)
            .unwrap_or(false)
    }

    fn new_vote(
        local_vote_state: &VoteState,
        slot: u64,
        hash: Hash,
        last_bank_slot: Option<Slot>,
    ) -> (Vote, usize) {
        let mut local_vote_state = local_vote_state.clone();
        let vote = Vote::new(vec![slot], hash);
        local_vote_state.process_vote_unchecked(&vote);
        let slots = if let Some(last_bank_slot) = last_bank_slot {
            local_vote_state
                .votes
                .iter()
                .map(|v| v.slot)
                .skip_while(|s| *s <= last_bank_slot)
                .collect()
        } else {
            local_vote_state.votes.iter().map(|v| v.slot).collect()
        };
        trace!(
            "new vote with {:?} {:?} {:?}",
            last_bank_slot,
            slots,
            local_vote_state.votes
        );
        (Vote::new(slots, hash), local_vote_state.votes.len() - 1)
    }

    fn last_bank_vote(bank: &Bank, vote_account_pubkey: &Pubkey) -> Option<Slot> {
        let vote_account = bank.vote_accounts().get(vote_account_pubkey)?.1.clone();
        let bank_vote_state = VoteState::deserialize(&vote_account.data).ok()?;
        bank_vote_state.votes.iter().map(|v| v.slot).last()
    }

    pub fn new_vote_from_bank(&self, bank: &Bank, vote_account_pubkey: &Pubkey) -> (Vote, usize) {
        let last_vote = Self::last_bank_vote(bank, vote_account_pubkey);
        Self::new_vote(&self.lockouts, bank.slot(), bank.hash(), last_vote)
    }

    pub fn record_bank_vote(&mut self, vote: Vote) -> Option<Slot> {
        let slot = *vote.slots.last().unwrap_or(&0);
        trace!("{} record_vote for {}", self.node_pubkey, slot);
        let root_slot = self.lockouts.root_slot;
        self.lockouts.process_vote_unchecked(&vote);
        self.last_vote = vote;

        datapoint_info!(
            "tower-vote",
            ("latest", slot, i64),
            ("root", self.lockouts.root_slot.unwrap_or(0), i64)
        );
        if root_slot != self.lockouts.root_slot {
            Some(self.lockouts.root_slot.unwrap())
        } else {
            None
        }
    }

    pub fn record_vote(&mut self, slot: Slot, hash: Hash) -> Option<Slot> {
        let vote = Vote::new(vec![slot], hash);
        self.record_bank_vote(vote)
    }

    pub fn last_vote(&self) -> Vote {
        self.last_vote.clone()
    }

    pub fn last_vote_and_timestamp(&mut self) -> Vote {
        let mut last_vote = self.last_vote();
        let current_slot = last_vote.slots.iter().max().unwrap_or(&0);
        last_vote.timestamp = self.maybe_timestamp(*current_slot);
        last_vote
    }

    pub fn root(&self) -> Option<Slot> {
        self.lockouts.root_slot
    }

    // a slot is not recent if it's older than the newest vote we have
    pub fn is_recent(&self, slot: u64) -> bool {
        if let Some(last_vote) = self.lockouts.votes.back() {
            if slot <= last_vote.slot {
                return false;
            }
        }
        true
    }

    pub fn has_voted(&self, slot: u64) -> bool {
        for vote in &self.lockouts.votes {
            if vote.slot == slot {
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
                assert!(ancestors[&slot].contains(&root_slot));
            }
        }

        false
    }

    pub(crate) fn check_switch_threshold(
        &self,
        switch_slot: u64,
        ancestors: &HashMap<Slot, HashSet<u64>>,
        descendants: &HashMap<Slot, HashSet<u64>>,
        progress: &ProgressMap,
        total_stake: u64,
        epoch_vote_accounts: &HashMap<Pubkey, (u64, Account)>,
    ) -> SwitchForkDecision {
        self.last_vote()
            .slots
            .last()
            .map(|last_vote| {
                let last_vote_ancestors = ancestors.get(&last_vote).unwrap();
                let switch_slot_ancestors = ancestors.get(&switch_slot).unwrap();

                if switch_slot == *last_vote || switch_slot_ancestors.contains(last_vote) {
                    // If the `switch_slot is a descendant of the last vote,
                    // no switching proof is necessary
                    return SwitchForkDecision::NoSwitch;
                }

                // Should never consider switching to an ancestor
                // of your last vote
                assert!(!last_vote_ancestors.contains(&switch_slot));

                // By this point, we know the `switch_slot` is on a different fork
                // (is neither an ancestor nor descendant of `last_vote`), so a
                // switching proof is necessary
                let switch_proof = Hash::default();
                let mut locked_out_stake = 0;
                let mut locked_out_vote_accounts = HashSet::new();
                for (candidate_slot, descendants) in descendants.iter() {
                    // 1) Only consider lockouts a tips of forks as that
                    //    includes all ancestors of that fork.
                    // 2) Don't consider lockouts on the `last_vote` itself
                    // 3) Don't consider lockouts on any descendants of
                    //    `last_vote`
                    if !descendants.is_empty()
                        || candidate_slot == last_vote
                        || ancestors
                            .get(&candidate_slot)
                            .expect(
                                "empty descendants implies this is a child, not parent of root, so must
                                exist in the ancestors map",
                            )
                            .contains(last_vote)
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
                    for (_, value) in lockout_intervals.range((Included(last_vote), Unbounded)) {
                        for (lockout_interval_start, vote_account_pubkey) in value {
                            // Only count lockouts on slots that are:
                            // 1) Not ancestors of `last_vote`
                            // 2) Not from before the current root as we can't determine if
                            // anything before the root was an ancestor of `last_vote` or not
                            if !last_vote_ancestors.contains(lockout_interval_start)
                                // The check if the key exists in the ancestors map
                                // is equivalent to checking if the key is above the
                                // current root.
                                && ancestors.contains_key(lockout_interval_start)
                                && !locked_out_vote_accounts.contains(vote_account_pubkey)
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
                    SwitchForkDecision::FailedSwitchThreshold
                }
            })
            .unwrap_or(SwitchForkDecision::NoSwitch)
    }

    pub fn check_vote_stake_threshold(
        &self,
        slot: Slot,
        stake_lockouts: &HashMap<u64, StakeLockout>,
        total_staked: u64,
    ) -> bool {
        let mut lockouts = self.lockouts.clone();
        lockouts.process_slot_vote_unchecked(slot);
        let vote = lockouts.nth_recent_vote(self.threshold_depth);
        if let Some(vote) = vote {
            if let Some(fork_stake) = stake_lockouts.get(&vote.slot) {
                let lockout = fork_stake.stake as f64 / total_staked as f64;
                trace!(
                    "fork_stake slot: {}, vote slot: {}, lockout: {} fork_stake: {} total_stake: {}",
                    slot, vote.slot, lockout, fork_stake.stake, total_staked
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
    pub(crate) fn update_ancestor_lockouts(
        stake_lockouts: &mut HashMap<Slot, StakeLockout>,
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
            let entry = &mut stake_lockouts.entry(slot).or_default();
            entry.lockout += vote.lockout();
        }
    }

    pub(crate) fn find_heaviest_bank(
        bank_forks: &RwLock<BankForks>,
        node_pubkey: &Pubkey,
    ) -> Option<Arc<Bank>> {
        let ancestors = bank_forks.read().unwrap().ancestors();
        let mut bank_weights: Vec<_> = bank_forks
            .read()
            .unwrap()
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
    fn update_ancestor_stakes(
        stake_lockouts: &mut HashMap<Slot, StakeLockout>,
        slot: Slot,
        lamports: u64,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
    ) {
        // If there's no ancestors, that means this slot must be from
        // before the current root, so ignore this slot
        let vote_slot_ancestors = ancestors.get(&slot);
        if vote_slot_ancestors.is_none() {
            return;
        }
        let mut slot_with_ancestors = vec![slot];
        slot_with_ancestors.extend(vote_slot_ancestors.unwrap());
        for slot in slot_with_ancestors {
            let entry = &mut stake_lockouts.entry(slot).or_default();
            entry.stake += lamports;
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

    fn initialize_lockouts_from_bank_forks(
        &mut self,
        vote_account_pubkey: &Pubkey,
        root: Slot,
        heaviest_bank: &Bank,
    ) {
        if let Some((_stake, vote_account)) = heaviest_bank.vote_accounts().get(vote_account_pubkey)
        {
            let mut vote_state = VoteState::deserialize(&vote_account.data)
                .expect("vote_account isn't a VoteState?");
            vote_state.root_slot = Some(root);
            vote_state.votes.retain(|v| v.slot > root);
            trace!(
                "{} lockouts initialized to {:?}",
                self.node_pubkey,
                vote_state
            );
            assert_eq!(
                vote_state.node_pubkey, self.node_pubkey,
                "vote account's node_pubkey doesn't match",
            );
            self.lockouts = vote_state;
        }
    }

    fn maybe_timestamp(&mut self, current_slot: Slot) -> Option<UnixTimestamp> {
        if self.last_timestamp.slot == 0
            || self.last_timestamp.slot < (current_slot - (current_slot % TIMESTAMP_SLOT_INTERVAL))
        {
            let timestamp = Utc::now().timestamp();
            self.last_timestamp = BlockTimestamp {
                slot: current_slot,
                timestamp,
            };
            Some(timestamp)
        } else {
            None
        }
    }
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
    use solana_ledger::bank_forks::BankForks;
    use solana_runtime::{
        bank::Bank,
        genesis_utils::{
            create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
        },
    };
    use solana_sdk::{
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    };
    use solana_vote_program::{
        vote_state::{Vote, VoteStateVersions, MAX_LOCKOUT_HISTORY},
        vote_transaction,
    };
    use std::{
        collections::HashMap,
        rc::Rc,
        sync::RwLock,
        {thread::sleep, time::Duration},
    };
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
                &tower,
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
                let node_keypair = Keypair::new();
                let vote_keypair = Keypair::new();
                let stake_keypair = Keypair::new();
                let node_pubkey = node_keypair.pubkey();
                (
                    node_pubkey,
                    ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
                )
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
        } = create_genesis_config_with_vote_accounts(1_000_000_000, &validator_keypairs, stake);

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

    fn gen_stakes(stake_votes: &[(u64, &[u64])]) -> Vec<(Pubkey, (u64, Account))> {
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
            stakes.push((Pubkey::new_rand(), (*lamports, account)));
        }
        stakes
    }

    #[test]
    fn test_to_vote_instruction() {
        let vote = Vote::default();
        let mut decision = SwitchForkDecision::FailedSwitchThreshold;
        assert!(decision
            .to_vote_instruction(vote.clone(), &Pubkey::default(), &Pubkey::default())
            .is_none());
        decision = SwitchForkDecision::NoSwitch;
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
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        let descendants = vote_simulator.bank_forks.read().unwrap().descendants();
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
            SwitchForkDecision::NoSwitch
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
            SwitchForkDecision::FailedSwitchThreshold
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
            SwitchForkDecision::FailedSwitchThreshold
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
            SwitchForkDecision::FailedSwitchThreshold
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
            SwitchForkDecision::FailedSwitchThreshold
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

        // If we set a root, then any lockout intervals below the root shouldn't
        // count toward the switch threshold. This means the other validator's
        // vote lockout no longer counts
        vote_simulator.set_root(43);
        assert_eq!(
            tower.check_switch_threshold(
                110,
                &vote_simulator.bank_forks.read().unwrap().ancestors(),
                &vote_simulator.bank_forks.read().unwrap().descendants(),
                &vote_simulator.progress,
                total_stake,
                bank0.epoch_vote_accounts(0).unwrap(),
            ),
            SwitchForkDecision::FailedSwitchThreshold
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
        // on another fork, so switching should suceed
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
        let vote_accounts = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(next_unlocked_slot)
            .unwrap()
            .vote_accounts();
        let observed = vote_accounts.get(&vote_pubkey).unwrap();
        let state = VoteState::from(&observed.1).unwrap();
        info!("observed tower: {:#?}", state.votes);

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
        let account_latest_votes: Vec<(Pubkey, Slot)> =
            accounts.iter().map(|(pubkey, _)| (*pubkey, 0)).collect();

        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        let ComputedBankState {
            stake_lockouts,
            total_staked,
            bank_weight,
            mut pubkey_votes,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            1,
            accounts.into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        assert_eq!(stake_lockouts[&0].stake, 2);
        assert_eq!(stake_lockouts[&0].lockout, 2 + 2 + 4 + 4);
        assert_eq!(total_staked, 2);
        pubkey_votes.sort();
        assert_eq!(pubkey_votes, account_latest_votes);

        // Each acccount has 1 vote in it. After simulating a vote in collect_vote_lockouts,
        // the account will have 2 votes, with lockout 2 + 4 = 6. So expected weight for
        // two acccounts is 2 * 6 = 12
        assert_eq!(bank_weight, 12)
    }

    #[test]
    fn test_collect_vote_lockouts_root() {
        let votes: Vec<u64> = (0..MAX_LOCKOUT_HISTORY as u64).collect();
        //two accounts voting for slots 0..MAX_LOCKOUT_HISTORY with 1 token staked
        let mut accounts = gen_stakes(&[(1, &votes), (1, &votes)]);
        accounts.sort_by_key(|(pk, _)| *pk);
        let account_latest_votes: Vec<(Pubkey, Slot)> = accounts
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
            stake_lockouts,
            bank_weight,
            mut pubkey_votes,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            MAX_LOCKOUT_HISTORY as u64,
            accounts.into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        for i in 0..MAX_LOCKOUT_HISTORY {
            assert_eq!(stake_lockouts[&(i as u64)].stake, 2);
        }

        // should be the sum of all the weights for root
        assert!(stake_lockouts[&0].lockout > (2 * (1 << MAX_LOCKOUT_HISTORY)));
        assert_eq!(bank_weight, expected_bank_weight);
        pubkey_votes.sort();
        assert_eq!(pubkey_votes, account_latest_votes);
    }

    #[test]
    fn test_check_vote_threshold_without_votes() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(tower.check_vote_stake_threshold(0, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_no_skip_lockout_with_new_root() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(4, 0.67);
        let mut stakes = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY as u64 + 1) {
            stakes.insert(
                i,
                StakeLockout {
                    stake: 1,
                    lockout: 8,
                },
            );
            tower.record_vote(i, Hash::default());
        }
        assert!(!tower.check_vote_stake_threshold(MAX_LOCKOUT_HISTORY as u64 + 1, &stakes, 2,));
    }

    #[test]
    fn test_is_slot_confirmed_not_enough_stake_failure() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
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
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
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
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        assert!(!tower.check_vote_stake_threshold(1, &stakes, 2));
    }
    #[test]
    fn test_check_vote_threshold_above_threshold() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        assert!(tower.check_vote_stake_threshold(1, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_after_pop() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
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
        let stakes = vec![
            (
                0,
                StakeLockout {
                    stake: 1,
                    lockout: 8,
                },
            ),
            (
                1,
                StakeLockout {
                    stake: 2,
                    lockout: 8,
                },
            ),
        ]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        assert!(tower.check_vote_stake_threshold(6, &stakes, 2,));
    }

    #[test]
    fn test_stake_is_updated_for_entire_branch() {
        let mut stake_lockouts = HashMap::new();
        let mut account = Account::default();
        account.lamports = 1;
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let ancestors: HashMap<u64, HashSet<u64>> = [(2u64, set)].iter().cloned().collect();
        Tower::update_ancestor_stakes(&mut stake_lockouts, 2, account.lamports, &ancestors);
        assert_eq!(stake_lockouts[&0].stake, 1);
        assert_eq!(stake_lockouts[&1].stake, 1);
        assert_eq!(stake_lockouts[&2].stake, 1);
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
            stake_lockouts,
            total_staked,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            vote_to_evaluate,
            accounts.clone().into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        assert!(tower.check_vote_stake_threshold(vote_to_evaluate, &stake_lockouts, total_staked,));

        // CASE 2: Now we want to evaluate a vote for slot VOTE_THRESHOLD_DEPTH + 1. This slot
        // will expire the vote in one of the vote accounts, so we should have insufficient
        // stake to pass the threshold
        let vote_to_evaluate = VOTE_THRESHOLD_DEPTH as u64 + 1;
        let ComputedBankState {
            stake_lockouts,
            total_staked,
            ..
        } = Tower::collect_vote_lockouts(
            &Pubkey::default(),
            vote_to_evaluate,
            accounts.into_iter(),
            &ancestors,
            &mut PubkeyReferences::default(),
        );
        assert!(!tower.check_vote_stake_threshold(vote_to_evaluate, &stake_lockouts, total_staked,));
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
        assert_eq!(expected, tower.last_vote())
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
        assert!(tower.maybe_timestamp(TIMESTAMP_SLOT_INTERVAL).is_some());
        let BlockTimestamp { slot, timestamp } = tower.last_timestamp;

        assert_eq!(tower.maybe_timestamp(1), None);
        assert_eq!(tower.maybe_timestamp(slot), None);
        assert_eq!(tower.maybe_timestamp(slot + 1), None);

        sleep(Duration::from_secs(1));
        assert!(tower
            .maybe_timestamp(slot + TIMESTAMP_SLOT_INTERVAL + 1)
            .is_some());
        assert!(tower.last_timestamp.timestamp > timestamp);
    }
}
