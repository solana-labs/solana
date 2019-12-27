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
                fork_contents.retain(|slot| frozen_banks.contains_key(slot));

                let mut new_fork = Fork {
                    heaviest_slot: *slot,
                    fork_contents: vec![],
                };

                // If fork weights are same, we want to take lowest slot
                let current_best_weight = progress
                    .get(&new_fork.heaviest_slot)
                    .unwrap()
                    .fork_stats
                    .fork_weight;
                for slot in fork_contents.iter().skip(1) {
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

#[cfg(test)]
mod tests {
    use crate::fork_selector::{Fork, ForkSelector, OptimalForkSelector};
    use crate::replay_stage_ds::ForkProgress;
    use solana_runtime::bank::Bank;
    use solana_sdk::account::Account;
    use solana_sdk::clock::Slot;
    use solana_sdk::genesis_config::GenesisConfig;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::{HashMap, HashSet};
    use std::iter::FromIterator;
    use std::sync::Arc;

    #[test]
    fn test_optimal_extract_forks() {
        // Testing fork optimal fork extraction

        //                       /-> 10 -> 11
        //             /-> 6 -> 7
        //            /          \-> 14 -> 15
        // 3 -> 4 -> 5
        //            \-> 8 -> 9
        //                       \-> 12 -> 13
        //

        let mut genesis_config = GenesisConfig::default();
        genesis_config
            .accounts
            .insert(Pubkey::default(), Account::new(20, 20, &Pubkey::default()));
        let genesis_bank = Arc::new(Bank::new(&genesis_config));

        let mut frozen_banks: HashMap<Slot, Arc<Bank>> = HashMap::new();

        let highest_fork_weight_slots: HashMap<u64, u128> =
            vec![(14 as u64, 5 as u128), (15, 5), (13, 5), (11, 4)]
                .drain(..)
                .collect();

        let mut progress: HashMap<u64, ForkProgress> = HashMap::new();

        // Intentionally starting with 4, to cover a case, where slot is part of ancestor set, but not of frozen banks
        // (Because we rooted at 4)
        for i in 4..=15 {
            frozen_banks.insert(
                i,
                Arc::new(Bank::new_from_parent(&genesis_bank, &Pubkey::default(), i)),
            );

            let mut fork_progress = ForkProgress::new(i, Hash::default());

            if highest_fork_weight_slots.contains_key(&i) {
                fork_progress.fork_stats.fork_weight = highest_fork_weight_slots[&i];
            }

            fork_progress.fork_stats.computed = true;

            progress.insert(i, fork_progress);
        }

        let mut ancestors: HashMap<u64, HashSet<u64>> = HashMap::new();
        let mut descendants: HashMap<u64, HashSet<u64>> = HashMap::new();

        ancestors.insert(3, HashSet::new());
        descendants.insert(3, (4..=15).collect());

        ancestors.insert(4, (3..4).collect());
        descendants.insert(4, HashSet::from_iter(5..=15));

        ancestors.insert(5, (3..5).collect());
        descendants.insert(5, (6..=15).collect());

        ancestors.insert(6, (3..6).collect());
        descendants.insert(6, vec![7 as u64, 10, 11, 14, 15].drain(..).collect());

        ancestors.insert(7, (3..7).collect());
        descendants.insert(7, vec![10 as u64, 11, 14, 15].drain(..).collect());

        ancestors.insert(10, (3..=7).collect());
        descendants.insert(10, vec![11 as u64].drain(..).collect());

        ancestors.insert(11, vec![3 as u64, 4, 5, 6, 7, 10].drain(..).collect());
        descendants.insert(11, HashSet::default());

        ancestors.insert(14, (3..=7).collect());
        descendants.insert(14, vec![15 as u64].drain(..).collect());

        ancestors.insert(15, vec![3 as u64, 4, 5, 6, 7, 14].drain(..).collect());
        descendants.insert(15, HashSet::default());

        ancestors.insert(8, (3..=5).collect());
        descendants.insert(8, vec![9 as u64, 12, 13].drain(..).collect());

        ancestors.insert(9, vec![3 as u64, 4, 5, 8].drain(..).collect());
        descendants.insert(9, vec![12 as u64, 13].drain(..).collect());

        ancestors.insert(12, vec![3 as u64, 4, 5, 8, 9].drain(..).collect());
        descendants.insert(12, vec![13 as u64].drain(..).collect());

        ancestors.insert(13, vec![3 as u64, 4, 5, 8, 9, 12].drain(..).collect());
        descendants.insert(13, HashSet::default());

        let optimal_fork_selector = OptimalForkSelector::default();
        let forks =
            optimal_fork_selector.extract_forks(&frozen_banks, &ancestors, &descendants, &progress);

        let fork_map: HashMap<Slot, Vec<u64>> = forks
            .iter()
            .map(|f| (f.heaviest_slot, f.fork_contents.clone()))
            .collect();

        assert_eq!(fork_map.len(), 3);

        assert_eq!(fork_map.contains_key(&11), true);
        assert_eq!(fork_map[&11], vec![11, 10, 7, 6, 5, 4]);

        assert_eq!(fork_map.contains_key(&13), true);
        assert_eq!(fork_map[&13], vec![13, 12, 9, 8, 5, 4]);

        // Since 15 and 14 have same weight, we have picked 14
        assert_eq!(fork_map.contains_key(&14), true);
        assert_eq!(fork_map[&14], vec![15, 14, 7, 6, 5, 4]);
    }

    #[test]
    fn test_optimal_find_best_fork() {
        //                       /-> 10 -> 11
        //             /-> 6 -> 7
        //            /          \-> 14 -> 15
        // 3 -> 4 -> 5
        //            \-> 8 -> 9
        //                       \-> 12 -> 13
        //

        let forks = vec![
            Fork {
                heaviest_slot: 14,
                fork_contents: vec![15, 14, 7, 6, 5, 4],
            },
            Fork {
                heaviest_slot: 13,
                fork_contents: vec![13, 12, 9, 8, 5, 4],
            },
            Fork {
                heaviest_slot: 11,
                fork_contents: vec![11, 10, 7, 6, 5, 4],
            },
        ];

        let highest_fork_weight_slots: HashMap<u64, u128> =
            vec![(14 as u64, 5 as u128), (15, 5), (13, 5), (11, 4)]
                .drain(..)
                .collect();

        let mut progress: HashMap<u64, ForkProgress> = HashMap::new();

        // Intentionally starting with 4, to cover a case, where slot is part of ancestor set, but not of frozen banks
        // (Because we rooted at 4)
        for i in 4..=15 {
            let mut fork_progress = ForkProgress::new(i, Hash::default());

            if highest_fork_weight_slots.contains_key(&i) {
                fork_progress.fork_stats.fork_weight = highest_fork_weight_slots[&i];
            }

            fork_progress.fork_stats.computed = true;

            progress.insert(i, fork_progress);
        }

        let optimal_fork_selector = OptimalForkSelector::default();

        let best_fork = optimal_fork_selector.find_best_fork(forks, &progress);
        assert!(best_fork.is_some());

        let best_fork = best_fork.unwrap();
        // best fork would be 13 as 13, 14 and 15 has same weight, so we will choose lower slot
        assert_eq!(best_fork.heaviest_slot, 13);
        assert_eq!(best_fork.fork_contents, vec![13 as u64, 12, 9, 8, 5, 4]);
    }
}
