use crate::replay_stage_ds::ForkProgress;
use solana_runtime::bank::Bank;
use solana_sdk::clock::Slot;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone)]
pub struct Fork {
    pub heaviest_slot: Slot,
    pub fork_contents: Vec<Slot>,
}

pub trait ForkSelector {
    /// extracts fork structure from set of frozen banks,
    /// ascendants and descendants
    fn extract_forks(
        &self,
        frozen_banks: &HashMap<Slot, Arc<Bank>>,
        ancestors: &HashMap<u64, HashSet<u64>>,
        descendants: &HashMap<u64, HashSet<u64>>,
        progress: &HashMap<u64, ForkProgress>,
    ) -> Vec<Fork>;

    /// finds best fork out of vector of fork
    fn find_best_fork(
        &self,
        forks: Vec<Fork>,
        progress: &HashMap<u64, ForkProgress>,
    ) -> Option<Fork>;
}

#[derive(Default)]
pub struct OptimalForkSelector {}

impl ForkSelector for OptimalForkSelector {
    fn extract_forks(
        &self,
        frozen_banks: &HashMap<Slot, Arc<Bank>>,
        ancestors: &HashMap<u64, HashSet<u64>>,
        descendants: &HashMap<u64, HashSet<u64>>,
        progress: &HashMap<u64, ForkProgress>,
    ) -> Vec<Fork> {
        let mut forks: Vec<Fork> = vec![];

        for (_i, slot) in ancestors.keys().enumerate() {
            if (!descendants.contains_key(slot) || descendants[slot].is_empty())
                && frozen_banks.contains_key(slot)
            {
                let mut fork_contents: Vec<u64> = ancestors[slot].iter().copied().collect();
                fork_contents.push(*slot);
                fork_contents.sort_by(|a, b| a.cmp(b).reverse());

                let mut new_fork = Fork {
                    heaviest_slot: *slot,
                    fork_contents: vec![],
                };

                // If weights are same, we want to take lowest slot
                let current_best_weight = progress
                    .get(&new_fork.heaviest_slot)
                    .unwrap()
                    .fork_stats
                    .fork_weight;
                for slot in fork_contents.iter().rev().skip(1) {
                    let fork_weight = progress.get(slot).unwrap().fork_stats.fork_weight;
                    if fork_weight >= current_best_weight {
                        new_fork.heaviest_slot = *slot;
                    } else {
                        break;
                    }
                }

                new_fork.fork_contents = fork_contents;

                forks.push(new_fork);
            }
        }

        forks
    }

    fn find_best_fork(
        &self,
        mut forks: Vec<Fork>,
        progress: &HashMap<u64, ForkProgress>,
    ) -> Option<Fork> {
        if forks.is_empty() {
            return None;
        }

        // Highest weight, lowest slot
        forks.sort_by(|fork_a, fork_b| {
            let fork_a_stats = &progress.get(&fork_a.heaviest_slot).unwrap().fork_stats;
            let fork_b_stats = &progress.get(&fork_b.heaviest_slot).unwrap().fork_stats;

            if fork_a_stats.fork_weight != fork_b_stats.fork_weight {
                fork_a_stats.fork_weight.cmp(&fork_b_stats.fork_weight)
            } else {
                fork_a.heaviest_slot.cmp(&fork_b.heaviest_slot).reverse()
            }
        });

        Some(forks.last().unwrap().clone())
    }
}

mod tests {
    #[test]
    fn test_optimal_extract_forks() {}

    #[test]
    fn test_optimal_find_best_fork() {}
}
