use crate::{
    consensus::{ComputedBankState, Tower},
    fork_choice::ForkChoice,
    heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
    progress_map::ProgressMap,
};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

impl ForkChoice for HeaviestSubtreeForkChoice {
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
