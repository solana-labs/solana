use crate::{
    consensus::Tower,
    fork_choice::{self, ComputedBankState, ForkChoice},
    heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
    progress_map::ProgressMap,
    pubkey_references::PubkeyReferences,
};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{account::Account, clock::Slot, pubkey::Pubkey};
use solana_vote_program::vote_state::VoteState;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, RwLock},
};

impl ForkChoice for HeaviestSubtreeForkChoice {
    fn collect_vote_lockouts<F>(
        &self,
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
                    .or_insert_with(|| vec![])
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
            // Add the latest vote to update the `heaviest_subtree_fork_choice`
            if let Some(latest_vote) = vote_state.votes.back() {
                pubkey_votes.push((key, latest_vote.slot));
            }

            vote_state.process_slot_vote_unchecked(bank_slot);

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
                fork_choice::update_ancestor_stakes(
                    &mut stake_lockouts,
                    vote.slot,
                    lamports,
                    ancestors,
                );
            }
            total_staked += lamports;
        }

        ComputedBankState {
            stake_lockouts,
            bank_weight: 0,
            total_staked,
            lockout_intervals,
            pubkey_votes,
        }
    }

    fn compute_bank_stats(
        &mut self,
        bank: &Bank,
        _tower: &Tower,
        _progress: &mut ProgressMap,
        computed_bank_stats: &ComputedBankState,
    ) {
        let ComputedBankState { pubkey_votes, .. } = computed_bank_stats;

        // Update `heaviest_subtree_fork_choice` to find the best fork to build on
        let best_overall_slot = self.add_votes(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        datapoint_info!(
            "best_slot",
            ("slot", bank.slot(), i64),
            ("best_slot", best_overall_slot, i64),
        );
    }

    // Returns:
    // 1) The heaviest overall bbank
    // 2) The heavest bank on the same fork as the last vote (doesn't require a
    // switching proof to vote for)
    fn select_forks(
        &self,
        _frozen_banks: &[Arc<Bank>],
        tower: &Tower,
        _progress: &ProgressMap,
        _ancestors: &HashMap<u64, HashSet<u64>>,
        bank_forks: &RwLock<BankForks>,
    ) -> (Arc<Bank>, Option<Arc<Bank>>) {
        let last_vote = tower.last_vote().slots.last().cloned();
        let heaviest_slot_on_same_voted_fork = last_vote.map(|last_vote| {
            let heaviest_slot_on_same_voted_fork =
                self.best_slot(last_vote).expect("last_vote is a frozen bank so must have been added to heaviest_subtree_fork_choice at time of freezing");
            if heaviest_slot_on_same_voted_fork == last_vote {
                None
            } else {
                Some(heaviest_slot_on_same_voted_fork)
            }
        }).unwrap_or(None);
        let heaviest_slot = self.best_overall_slot();
        let r_bank_forks = bank_forks.read().unwrap();
        (
            r_bank_forks.get(heaviest_slot).unwrap().clone(),
            heaviest_slot_on_same_voted_fork.map(|heaviest_slot_on_same_voted_fork| {
                r_bank_forks
                    .get(heaviest_slot_on_same_voted_fork)
                    .unwrap()
                    .clone()
            }),
        )
    }
}
