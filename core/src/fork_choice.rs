use crate::{
    consensus::{StakeLockout, SwitchForkDecision, Tower},
    progress_map::{LockoutIntervals, ProgressMap},
    pubkey_references::PubkeyReferences,
    replay_stage::HeaviestForkFailures,
};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{account::Account, clock::Slot, pubkey::Pubkey};
use solana_vote_program::vote_state::Lockout;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

pub(crate) struct SelectVoteAndResetForkResult {
    pub vote_bank: Option<(Arc<Bank>, SwitchForkDecision)>,
    pub reset_bank: Option<Arc<Bank>>,
    pub heaviest_fork_failures: Vec<HeaviestForkFailures>,
}

pub(crate) struct ComputedBankState {
    pub stake_lockouts: HashMap<Slot, StakeLockout>,
    pub total_staked: u64,
    pub bank_weight: u128,
    pub lockout_intervals: LockoutIntervals,
    pub pubkey_votes: Vec<(Pubkey, Slot)>,
}

pub(crate) trait ForkChoice {
    fn collect_vote_lockouts<F>(
        &self,
        node_pubkey: &Pubkey,
        bank_slot: u64,
        vote_accounts: F,
        ancestors: &HashMap<Slot, HashSet<u64>>,
        all_pubkeys: &mut PubkeyReferences,
    ) -> ComputedBankState
    where
        F: Iterator<Item = (Pubkey, (u64, Account))>;

    fn compute_bank_stats(
        &mut self,
        bank: &Bank,
        tower: &Tower,
        progress: &mut ProgressMap,
        computed_bank_stats: &ComputedBankState,
    );

    // Returns:
    // 1) The heaviest overall bbank
    // 2) The heavest bank on the same fork as the last vote (doesn't require a
    // switching proof to vote for)
    fn select_forks(
        &self,
        frozen_banks: &[Arc<Bank>],
        tower: &Tower,
        progress: &ProgressMap,
        ancestors: &HashMap<u64, HashSet<u64>>,
        bank_forks: &RwLock<BankForks>,
    ) -> (Arc<Bank>, Option<Arc<Bank>>);
}

/// Update stake for all the ancestors.
/// Note, stake is the same for all the ancestor.
pub(crate) fn update_ancestor_stakes(
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
