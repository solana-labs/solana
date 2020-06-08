use crate::{
    consensus::{ComputedBankState, Tower},
    fork_choice::ForkChoice,
    progress_map::{ForkStats, ProgressMap},
};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::timing;
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

#[derive(Default)]
pub struct BankWeightForkChoice {}

impl ForkChoice for BankWeightForkChoice {
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
