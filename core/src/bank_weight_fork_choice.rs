use crate::{
    consensus::Tower,
    fork_choice::{self, ComputedBankState, ForkChoice},
    progress_map::{ForkStats, ProgressMap},
    pubkey_references::PubkeyReferences,
};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{account::Account, clock::Slot, pubkey::Pubkey, timing};
use solana_vote_program::vote_state::{Lockout, VoteState, MAX_LOCKOUT_HISTORY};
use std::time::Instant;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, RwLock},
};

pub struct BankWeightForkChoice {}

impl ForkChoice for BankWeightForkChoice {
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
        let mut bank_weight = 0;
        // Tree of intervals of lockouts of the form [slot, slot + slot.lockout],
        // keyed by end of the range
        let mut lockout_intervals = BTreeMap::new();
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
            let start_root = vote_state.root_slot;

            vote_state.process_slot_vote_unchecked(bank_slot);

            for vote in &vote_state.votes {
                bank_weight += vote.lockout() as u128 * lamports as u128;
                fork_choice::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
            }

            if start_root != vote_state.root_slot {
                if let Some(root) = start_root {
                    let vote = Lockout {
                        confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                        slot: root,
                    };
                    trace!("ROOT: {}", vote.slot);
                    bank_weight += vote.lockout() as u128 * lamports as u128;
                    fork_choice::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
                }
            }
            if let Some(root) = vote_state.root_slot {
                let vote = Lockout {
                    confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                    slot: root,
                };
                bank_weight += vote.lockout() as u128 * lamports as u128;
                fork_choice::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
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
            bank_weight,
            lockout_intervals,
            pubkey_votes: vec![],
        }
    }

    fn compute_bank_stats(
        &mut self,
        bank: &Bank,
        _tower: &Tower,
        progress: &mut ProgressMap,
        computed_bank_stats: &ComputedBankState,
    ) {
        let bank_slot = bank.slot();
        // Only time progress map should be missing a bank slot
        // is if this node was the leader for this slot as those banks
        // are not replayed in replay_active_banks()
        let parent_weight = bank
            .parent()
            .and_then(|b| progress.get(&b.slot()))
            .map(|x| x.fork_stats.fork_weight)
            .unwrap_or(0);

        let stats = progress
            .get_fork_stats_mut(bank_slot)
            .expect("All frozen banks must exist in the Progress map");

        let ComputedBankState { bank_weight, .. } = computed_bank_stats;
        stats.weight = *bank_weight;
        stats.fork_weight = stats.weight + parent_weight;

        datapoint_info!(
            "bank_weight",
            ("slot", bank_slot, i64),
            // u128 too large for influx, convert to hex
            ("weight", format!("{:X}", stats.weight), String),
        );
        info!(
            "slot_weight: {} {} {} {}",
            bank_slot,
            stats.weight,
            stats.fork_weight,
            bank.parent().map(|b| b.slot()).unwrap_or(0)
        );
    }

    // Returns:
    // 1) The heaviest overall bank
    // 2) The heavest bank on the same fork as the last vote (doesn't require a
    // switching proof to vote for)
    fn select_forks(
        &self,
        frozen_banks: &[Arc<Bank>],
        tower: &Tower,
        progress: &ProgressMap,
        ancestors: &HashMap<u64, HashSet<u64>>,
        _bank_forks: &RwLock<BankForks>,
    ) -> (Arc<Bank>, Option<Arc<Bank>>) {
        let tower_start = Instant::now();
        assert!(!frozen_banks.is_empty());
        let num_frozen_banks = frozen_banks.len();

        trace!("frozen_banks {}", frozen_banks.len());
        let num_old_banks = frozen_banks
            .iter()
            .filter(|b| b.slot() < tower.root().unwrap_or(0))
            .count();

        let last_vote = tower.last_vote().slots.last().cloned();
        let mut heaviest_bank_on_same_fork = None;
        let mut heaviest_same_fork_weight = 0;
        let stats: Vec<&ForkStats> = frozen_banks
            .iter()
            .map(|bank| {
                // Only time progress map should be missing a bank slot
                // is if this node was the leader for this slot as those banks
                // are not replayed in replay_active_banks()
                let stats = progress
                    .get_fork_stats(bank.slot())
                    .expect("All frozen banks must exist in the Progress map");

                if let Some(last_vote) = last_vote {
                    if ancestors
                        .get(&bank.slot())
                        .expect("Entry in frozen banks must exist in ancestors")
                        .contains(&last_vote)
                    {
                        // Descendant of last vote cannot be locked out
                        assert!(!stats.is_locked_out);

                        // ancestors(slot) should not contain the slot itself,
                        // so we should never get the same bank as the last vote
                        assert_ne!(bank.slot(), last_vote);
                        // highest weight, lowest slot first. frozen_banks is sorted
                        // from least slot to greatest slot, so if two banks have
                        // the same fork weight, the lower slot will be picked
                        if stats.fork_weight > heaviest_same_fork_weight {
                            heaviest_bank_on_same_fork = Some(bank.clone());
                            heaviest_same_fork_weight = stats.fork_weight;
                        }
                    }
                }

                stats
            })
            .collect();
        let num_not_recent = stats.iter().filter(|s| !s.is_recent).count();
        let num_has_voted = stats.iter().filter(|s| s.has_voted).count();
        let num_empty = stats.iter().filter(|s| s.is_empty).count();
        let num_threshold_failure = stats.iter().filter(|s| !s.vote_threshold).count();
        let num_votable_threshold_failure = stats
            .iter()
            .filter(|s| s.is_recent && !s.has_voted && !s.vote_threshold)
            .count();

        let mut candidates: Vec<_> = frozen_banks.iter().zip(stats.iter()).collect();

        //highest weight, lowest slot first
        candidates.sort_by_key(|b| (b.1.fork_weight, 0i64 - b.0.slot() as i64));
        let rv = candidates
            .last()
            .expect("frozen banks was nonempty so candidates must also be nonempty");
        let ms = timing::duration_as_ms(&tower_start.elapsed());
        let weights: Vec<(u128, u64, u64)> = candidates
            .iter()
            .map(|x| (x.1.weight, x.0.slot(), x.1.block_height))
            .collect();
        debug!(
            "@{:?} tower duration: {:?} len: {}/{} weights: {:?}",
            timing::timestamp(),
            ms,
            candidates.len(),
            stats.iter().filter(|s| !s.has_voted).count(),
            weights,
        );
        datapoint_debug!(
            "replay_stage-select_forks",
            ("frozen_banks", num_frozen_banks as i64, i64),
            ("not_recent", num_not_recent as i64, i64),
            ("has_voted", num_has_voted as i64, i64),
            ("old_banks", num_old_banks as i64, i64),
            ("empty_banks", num_empty as i64, i64),
            ("threshold_failure", num_threshold_failure as i64, i64),
            (
                "votable_threshold_failure",
                num_votable_threshold_failure as i64,
                i64
            ),
            ("tower_duration", ms as i64, i64),
        );

        (rv.0.clone(), heaviest_bank_on_same_fork)
    }
}
