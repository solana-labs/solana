use solana_ledger::bank_forks::BankForks;
use solana_runtime::epoch_stakes::EpochStakes;
use solana_sdk::{clock::Slot, epoch_schedule::EpochSchedule, pubkey::Pubkey};
use std::collections::{BTreeMap, HashMap, HashSet};

pub type ForkWeight = usize;

struct ForkInfo {
    // Amount of stake that has voted for exactly this slot
    voted_at: ForkWeight,
    // Amount of stake that has voted for this slot and the subtree 
    // rooted at this slot
    voted_above: ForkWeight,
    best_child: Slot,
}

struct ForkWeightTracker {
    fork_infos: HashMap<Slot, ForkInfo>,
    latest_votes: HashMap<Pubkey, Slot>,
    root: Slot,
}

impl ForkWeightTracker {
    pub fn add_votes(
        &mut self,
        // newly updated votes on a fork
        pubkey_votes: &[(Pubkey, Slot)],
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
        // set of slots on this fork from highest to lowest
        ancestors: &[Slot],
    ) {
        let mut votes_by_slot: BTreeMap<Slot, u64> = BTreeMap::new();
        let mut old_votes: BTreeMap<Slot, u64> = BTreeMap::new();
        // Sort the `pubkey_votes` in a BTreeMap by the slot voted
        for (pubkey, slot) in pubkey_votes.iter() {
            let pubkey_latest_vote = self.latest_votes.get(pubkey).unwrap_or(0);
            // Filter out any votes or slots <= any slot this pubkey has
            // already voted for, we only care about the latest votes
            let old_vote = {
                if slot > pubkey_latest_vote {
                    self.latest_votes.insert(*pubkey, slot)
                } else {
                    continue;
                }
            };
            let epoch = epoch_schedule.get_epoch(*slot);
            let stake = epoch_stakes.get(epoch).vote_account_stake(pubkey);
            old_votes
                .entry(old_vote.unwrap())
                .and_modify(|total_stake| *total_stake += stake)
                .or_insert(stake);
            votes_by_slot
                .entry(*slot)
                .and_modify(|total_stake| *total_stake += stake)
                .or_insert(stake);
        }

        if votes_by_slot.is_empty() {
            return;
        }

        // `new_vote_stake_map` contains a mapping from a slot to the total stake that
        // confirms the branch of this fork rooted at that slot
        let mut aggregated_vote_stakes = vec![0; votes_by_slot.len()];
        votes_by_slot
            .iter()
            .rev()
            .enumerate()
            .fold(0, |sum, (i, (slot, stake))| {
                let confirmed_stake = sum + stake;
                aggregated_vote_stakes[i] = (slot, confirmed_stake);
                confirmed_stake
            });
        
        // Update `self.fork_infos` with new stake distribution
        self.update_fork_infos(ancestors, &aggregated_vote_stakes);

        // Remove old votes from the corresponding forks
        
    }

    pub fn update_root(&mut self, root: Slot, descendants: &HashMap<Slot, HashSet<Slot>>) {
        self.root = root;
        let root_descendants = descendants.get(&root).expect("Root must have descendants");
        self.fork_infos
            .retain(|slot, slot_info| *slot >= root && root_descendants.contains(slot));
    }

    pub fn compute_best(&self, slot: Slot, descendants: &HashMap<Slot, HashSet<Slot>>) {

    }

    // Update `self.fork_infos` with new stake distribution based on latest votes
    // on *one* fork
    fn update_fork_infos(
        &mut self, 
        // set of slots on this fork from highest to lowest
        ancestors: &[Slot],
        aggregated_vote_stakes: &[u64]
    ) {
        // Current index into `aggregated_vote_stakes`
        let mut aggregate_vote_index = 0;
        let mut stake_to_increase = 0;
        let mut prev_ancestor = 0;
        for ancestor in ancestors.iter() {
            // Ensure this list is sorted from greatest to lowest
            assert!(*ancestor > prev_ancestor);
            if ancestor < aggregated_vote_stakes[aggregate_vote_index].0 {
                stake_to_increase = aggregated_vote_stakes[aggregate_vote_index].1;
                aggregate_vote_index += 1;
            }

            if stake_to_increase > 0 {
                self.fork_infos
                    .entry(*ancestor)
                    .and_modify(|fork_info| fork_info.stake += stake_to_increase)
                    .or_insert(ForkInfo {
                        weight: stake_to_increase,
                        best_target: 
                    });
            }
        }
    }
}
