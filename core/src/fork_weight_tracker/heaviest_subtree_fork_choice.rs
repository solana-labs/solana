use crate::{
    consensus::{SwitchForkDecision, Tower},
    fork_choice::{self, ComputedBankState, ForkChoice, SelectVoteAndResetForkResult},
    fork_weight_tracker::ForkWeightTracker,
    progress_map::ProgressMap,
    pubkey_references::PubkeyReferences,
    replay_stage::HeaviestForkFailures,
};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{account::Account, clock::Slot, pubkey::Pubkey};
use solana_vote_program::vote_state::VoteState;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, RwLock},
};

impl ForkChoice for ForkWeightTracker {
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
            // Add the latest vote to update the `fork_weight_tracker`
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

        // Update `fork_weight_tracker` to find the best fork to build on
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
                self.best_slot(last_vote).expect("last_vote is a frozen bank so must have been added to fork_weight_tracker at time of freezing");
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

    // Given a heaviest bank, `heaviest_bank` and the next votable bank
    // `heaviest_bank_on_same_voted_fork` as the validator's last vote, return
    // a bank to vote on, a bank to reset to,
    fn select_vote_and_reset_forks(
        heaviest_bank: &Arc<Bank>,
        heaviest_bank_on_same_voted_fork: &Option<Arc<Bank>>,
        ancestors: &HashMap<u64, HashSet<u64>>,
        descendants: &HashMap<u64, HashSet<u64>>,
        progress: &ProgressMap,
        tower: &Tower,
    ) -> SelectVoteAndResetForkResult {
        // Try to vote on the actual heaviest fork. If the heaviest bank is
        // locked out or fails the threshold check, the validator will:
        // 1) Not continue to vote on current fork, waiting for lockouts to expire/
        //    threshold check to pass
        // 2) Will reset PoH to heaviest fork in order to make sure the heaviest
        //    fork is propagated
        // This above behavior should ensure correct voting and resetting PoH
        // behavior under all cases:
        // 1) The best "selected" bank is on same fork
        // 2) The best "selected" bank is on a different fork,
        //    switch_threshold fails
        // 3) The best "selected" bank is on a different fork,
        //    switch_threshold succceeds
        let mut failure_reasons = vec![];
        let selected_fork = {
            let switch_fork_decision = tower.check_switch_threshold(
                heaviest_bank.slot(),
                &ancestors,
                &descendants,
                &progress,
                heaviest_bank.total_epoch_stake(),
                heaviest_bank
                    .epoch_vote_accounts(heaviest_bank.epoch())
                    .expect("Bank epoch vote accounts must contain entry for the bank's own epoch"),
            );
            if switch_fork_decision == SwitchForkDecision::FailedSwitchThreshold {
                // If we can't switch, then reset to the the next votable
                // bank on the same fork as our last vote, but don't vote
                info!(
                    "Waiting to switch vote to {}, resetting to slot {:?} on same fork for now",
                    heaviest_bank.slot(),
                    heaviest_bank_on_same_voted_fork.as_ref().map(|b| b.slot())
                );
                failure_reasons.push(HeaviestForkFailures::FailedSwitchThreshold(
                    heaviest_bank.slot(),
                ));
                heaviest_bank_on_same_voted_fork
                    .as_ref()
                    .map(|b| (b, switch_fork_decision))
            } else {
                // If the switch threshold is observed, halt voting on
                // the current fork and attempt to vote/reset Poh to
                // the heaviest bank
                Some((heaviest_bank, switch_fork_decision))
            }
        };

        if let Some((bank, switch_fork_decision)) = selected_fork {
            let (is_locked_out, vote_threshold, is_leader_slot) = {
                let fork_stats = progress.get_fork_stats(bank.slot()).unwrap();
                let propagated_stats = &progress.get_propagated_stats(bank.slot()).unwrap();
                (
                    fork_stats.is_locked_out,
                    fork_stats.vote_threshold,
                    propagated_stats.is_leader_slot,
                )
            };

            let propagation_confirmed = is_leader_slot || progress.is_propagated(bank.slot());

            if is_locked_out {
                failure_reasons.push(HeaviestForkFailures::LockedOut(bank.slot()));
            }
            if !vote_threshold {
                failure_reasons.push(HeaviestForkFailures::FailedThreshold(bank.slot()));
            }
            if !propagation_confirmed {
                failure_reasons.push(HeaviestForkFailures::NoPropagatedConfirmation(bank.slot()));
            }

            if !is_locked_out
                && vote_threshold
                && propagation_confirmed
                && switch_fork_decision != SwitchForkDecision::FailedSwitchThreshold
            {
                info!("voting: {}", bank.slot());
                SelectVoteAndResetForkResult {
                    vote_bank: Some((bank.clone(), switch_fork_decision)),
                    reset_bank: Some(bank.clone()),
                    heaviest_fork_failures: failure_reasons,
                }
            } else {
                SelectVoteAndResetForkResult {
                    vote_bank: None,
                    reset_bank: Some(bank.clone()),
                    heaviest_fork_failures: failure_reasons,
                }
            }
        } else {
            SelectVoteAndResetForkResult {
                vote_bank: None,
                reset_bank: None,
                heaviest_fork_failures: failure_reasons,
            }
        }
    }
}
