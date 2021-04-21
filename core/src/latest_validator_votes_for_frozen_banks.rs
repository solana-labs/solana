use crate::heaviest_subtree_fork_choice::SlotHashKey;
use solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey};
use std::collections::{hash_map::Entry, HashMap};

#[derive(Default)]
pub(crate) struct LatestValidatorVotesForFrozenBanks {
    // TODO: Clean outdated/unstaked pubkeys from this list.
    max_frozen_votes: HashMap<Pubkey, (Slot, Vec<Hash>)>,
    // Pubkeys that had their `max_frozen_votes` updated since the last
    // fork choice update
    fork_choice_dirty_set: HashMap<Pubkey, (Slot, Vec<Hash>)>,
}

impl LatestValidatorVotesForFrozenBanks {
    // `frozen_hash.is_some()` if the bank with slot == `vote_slot` is frozen
    // Returns whether the vote was actually added, and the latest voted frozen slot
    pub(crate) fn check_add_vote(
        &mut self,
        vote_pubkey: Pubkey,
        vote_slot: Slot,
        frozen_hash: Option<Hash>,
    ) -> (bool, Option<Slot>) {
        let max_frozen_votes_entry = self.max_frozen_votes.entry(vote_pubkey);
        if let Some(frozen_hash) = frozen_hash {
            match max_frozen_votes_entry {
                Entry::Occupied(mut occupied_entry) => {
                    let (latest_frozen_vote_slot, latest_frozen_vote_hashes) =
                        occupied_entry.get_mut();
                    if vote_slot > *latest_frozen_vote_slot {
                        self.fork_choice_dirty_set
                            .insert(vote_pubkey, (vote_slot, vec![frozen_hash]));
                        *latest_frozen_vote_slot = vote_slot;
                        *latest_frozen_vote_hashes = vec![frozen_hash];
                        return (true, Some(vote_slot));
                    } else if vote_slot == *latest_frozen_vote_slot
                        && !latest_frozen_vote_hashes.contains(&frozen_hash)
                    {
                        let (_, dirty_frozen_hashes) =
                            self.fork_choice_dirty_set.entry(vote_pubkey).or_default();
                        assert!(!dirty_frozen_hashes.contains(&frozen_hash));
                        dirty_frozen_hashes.push(frozen_hash);
                        latest_frozen_vote_hashes.push(frozen_hash);
                        return (true, Some(vote_slot));
                    } else {
                        // We have newer votes for this validator, we don't care about this vote
                        return (false, Some(*latest_frozen_vote_slot));
                    }
                }

                Entry::Vacant(vacant_entry) => {
                    vacant_entry.insert((vote_slot, vec![frozen_hash]));
                    self.fork_choice_dirty_set
                        .insert(vote_pubkey, (vote_slot, vec![frozen_hash]));
                    return (true, Some(vote_slot));
                }
            }
        }

        // Non-frozen banks are not inserted because we only track frozen votes in this
        // struct
        (
            false,
            match max_frozen_votes_entry {
                Entry::Occupied(occupied_entry) => Some(occupied_entry.get().0),
                Entry::Vacant(_) => None,
            },
        )
    }

    pub(crate) fn take_votes_dirty_set(&mut self, root: Slot) -> Vec<(Pubkey, SlotHashKey)> {
        let new_votes = std::mem::take(&mut self.fork_choice_dirty_set);
        new_votes
            .into_iter()
            .filter(|(_, (slot, _))| *slot >= root)
            .flat_map(|(pk, (slot, hashes))| {
                hashes
                    .into_iter()
                    .map(|hash| (pk, (slot, hash)))
                    .collect::<Vec<(Pubkey, SlotHashKey)>>()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latest_validator_votes_for_frozen_banks_check_add_vote() {
        let mut latest_validator_votes_for_frozen_banks =
            LatestValidatorVotesForFrozenBanks::default();

        // Case 1: Non-frozen banks shouldn't be added
        let vote_pubkey = Pubkey::new_unique();
        let mut vote_slot = 1;
        let frozen_hash = None;
        assert_eq!(
            latest_validator_votes_for_frozen_banks.check_add_vote(
                vote_pubkey,
                vote_slot,
                frozen_hash,
            ),
            // Non-frozen bank isn't inserted, so should return None for
            // the highest voted frozen slot
            (false, None)
        );
        assert!(latest_validator_votes_for_frozen_banks
            .max_frozen_votes
            .is_empty());
        assert!(latest_validator_votes_for_frozen_banks
            .fork_choice_dirty_set
            .is_empty());

        // Case 2: Frozen vote should be added, but the same vote added again
        // shouldn't update state
        let num_repeated_iterations = 3;
        let frozen_hash = Some(Hash::new_unique());
        for i in 0..num_repeated_iterations {
            let expected_result = if i == 0 {
                (true, Some(vote_slot))
            } else {
                (false, Some(vote_slot))
            };
            assert_eq!(
                latest_validator_votes_for_frozen_banks.check_add_vote(
                    vote_pubkey,
                    vote_slot,
                    frozen_hash,
                ),
                expected_result
            );
            assert_eq!(
                *latest_validator_votes_for_frozen_banks
                    .max_frozen_votes
                    .get(&vote_pubkey)
                    .unwrap(),
                (vote_slot, vec![frozen_hash.unwrap()])
            );
            assert_eq!(
                *latest_validator_votes_for_frozen_banks
                    .fork_choice_dirty_set
                    .get(&vote_pubkey)
                    .unwrap(),
                (vote_slot, vec![frozen_hash.unwrap()])
            );
        }

        // Case 3: Adding duplicate vote for same slot should update the state
        let duplicate_frozen_hash = Some(Hash::new_unique());
        let all_frozen_hashes = vec![frozen_hash.unwrap(), duplicate_frozen_hash.unwrap()];
        assert_eq!(
            latest_validator_votes_for_frozen_banks.check_add_vote(
                vote_pubkey,
                vote_slot,
                duplicate_frozen_hash,
            ),
            (true, Some(vote_slot))
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .max_frozen_votes
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, all_frozen_hashes.clone())
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .fork_choice_dirty_set
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, all_frozen_hashes.clone())
        );

        // Case 4: Adding duplicate vote that is not frozen should not update the state
        let frozen_hash = None;
        assert_eq!(
            latest_validator_votes_for_frozen_banks.check_add_vote(
                vote_pubkey,
                vote_slot,
                frozen_hash,
            ),
            (false, Some(vote_slot))
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .max_frozen_votes
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, all_frozen_hashes.clone())
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .fork_choice_dirty_set
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, all_frozen_hashes.clone())
        );

        // Case 5: Adding a vote for a new higher slot that is not yet frozen
        // should not update the state
        let frozen_hash = None;
        let old_vote_slot = vote_slot;
        vote_slot += 1;
        assert_eq!(
            latest_validator_votes_for_frozen_banks.check_add_vote(
                vote_pubkey,
                vote_slot,
                frozen_hash,
            ),
            (false, Some(old_vote_slot))
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .max_frozen_votes
                .get(&vote_pubkey)
                .unwrap(),
            (old_vote_slot, all_frozen_hashes.clone())
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .fork_choice_dirty_set
                .get(&vote_pubkey)
                .unwrap(),
            (old_vote_slot, all_frozen_hashes)
        );

        // Case 6: Adding a vote for a new higher slot that *is* frozen
        // should upate the state
        let frozen_hash = Some(Hash::new_unique());
        assert_eq!(
            latest_validator_votes_for_frozen_banks.check_add_vote(
                vote_pubkey,
                vote_slot,
                frozen_hash,
            ),
            (true, Some(vote_slot))
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .max_frozen_votes
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, vec![frozen_hash.unwrap()])
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .fork_choice_dirty_set
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, vec![frozen_hash.unwrap()])
        );

        // Case 7: Adding a vote for a new pubkey should also update the state
        vote_slot += 1;
        let frozen_hash = Some(Hash::new_unique());
        let vote_pubkey = Pubkey::new_unique();
        assert_eq!(
            latest_validator_votes_for_frozen_banks.check_add_vote(
                vote_pubkey,
                vote_slot,
                frozen_hash,
            ),
            (true, Some(vote_slot))
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .max_frozen_votes
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, vec![frozen_hash.unwrap()])
        );
        assert_eq!(
            *latest_validator_votes_for_frozen_banks
                .fork_choice_dirty_set
                .get(&vote_pubkey)
                .unwrap(),
            (vote_slot, vec![frozen_hash.unwrap()])
        );
    }

    #[test]
    fn test_latest_validator_votes_for_frozen_banks_take_votes_dirty_set() {
        let mut latest_validator_votes_for_frozen_banks =
            LatestValidatorVotesForFrozenBanks::default();
        let num_validators = 10;

        let setup_dirty_set =
            |latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks| {
                (0..num_validators)
                    .flat_map(|vote_slot| {
                        let vote_pubkey = Pubkey::new_unique();
                        let frozen_hash1 = Hash::new_unique();
                        assert_eq!(
                            latest_validator_votes_for_frozen_banks.check_add_vote(
                                vote_pubkey,
                                vote_slot,
                                Some(frozen_hash1),
                            ),
                            // This vote slot was frozen, and is the highest slot inserted thus far,
                            // so the highest vote should be Some(vote_slot)
                            (true, Some(vote_slot))
                        );
                        // Add a duplicate
                        let frozen_hash2 = Hash::new_unique();
                        assert_eq!(
                            latest_validator_votes_for_frozen_banks.check_add_vote(
                                vote_pubkey,
                                vote_slot,
                                Some(frozen_hash2),
                            ),
                            // This vote slot was frozen, and is for a duplicate version of the highest slot
                            // inserted thus far, so the highest vote should be Some(vote_slot).
                            (true, Some(vote_slot))
                        );
                        vec![
                            (vote_pubkey, (vote_slot, frozen_hash1)),
                            (vote_pubkey, (vote_slot, frozen_hash2)),
                        ]
                    })
                    .collect()
            };

        // Taking all the dirty votes >= 0 will return everything
        let root = 0;
        let mut expected_dirty_set: Vec<(Pubkey, SlotHashKey)> =
            setup_dirty_set(&mut latest_validator_votes_for_frozen_banks);
        let mut votes_dirty_set_output =
            latest_validator_votes_for_frozen_banks.take_votes_dirty_set(root);
        votes_dirty_set_output.sort();
        expected_dirty_set.sort();
        assert_eq!(votes_dirty_set_output, expected_dirty_set);
        assert!(latest_validator_votes_for_frozen_banks
            .take_votes_dirty_set(0)
            .is_empty());

        // Taking all the firty votes >= num_validators - 1 will only return the last vote
        let root = num_validators - 1;
        let dirty_set = setup_dirty_set(&mut latest_validator_votes_for_frozen_banks);
        let mut expected_dirty_set: Vec<(Pubkey, SlotHashKey)> =
            dirty_set[dirty_set.len() - 2..dirty_set.len()].to_vec();
        let mut votes_dirty_set_output =
            latest_validator_votes_for_frozen_banks.take_votes_dirty_set(root);
        votes_dirty_set_output.sort();
        expected_dirty_set.sort();
        assert_eq!(votes_dirty_set_output, expected_dirty_set);
        assert!(latest_validator_votes_for_frozen_banks
            .take_votes_dirty_set(0)
            .is_empty());
    }
}
