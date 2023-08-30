use {
    crate::consensus::latest_validator_votes_for_frozen_banks::LatestValidatorVotesForFrozenBanks,
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    std::collections::{BTreeMap, HashMap},
};

#[derive(Default)]
pub struct UnfrozenGossipVerifiedVoteHashes {
    pub votes_per_slot: BTreeMap<Slot, HashMap<Hash, Vec<Pubkey>>>,
}

impl UnfrozenGossipVerifiedVoteHashes {
    // Update `latest_validator_votes_for_frozen_banks` if gossip has seen a newer vote
    // for a frozen bank.
    pub(crate) fn add_vote(
        &mut self,
        pubkey: Pubkey,
        vote_slot: Slot,
        hash: Hash,
        is_frozen: bool,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
    ) {
        // If this is a frozen bank, then we need to update the `latest_validator_votes_for_frozen_banks`
        let frozen_hash = if is_frozen { Some(hash) } else { None };
        let (was_added, latest_frozen_vote_slot) = latest_validator_votes_for_frozen_banks
            .check_add_vote(pubkey, vote_slot, frozen_hash, false);

        if !was_added
            && latest_frozen_vote_slot
                .map(|latest_frozen_vote_slot| vote_slot >= latest_frozen_vote_slot)
                // If there's no latest frozen vote slot yet, then we should also insert
                .unwrap_or(true)
        {
            // At this point it must be that:
            // 1) `vote_slot` was not yet frozen
            // 2) and `vote_slot` >= than the latest frozen vote slot.

            // Thus we want to record this vote for later, in case a slot with this `vote_slot` + hash gets
            // frozen later
            self.votes_per_slot
                .entry(vote_slot)
                .or_default()
                .entry(hash)
                .or_default()
                .push(pubkey);
        }
    }

    // Cleanup `votes_per_slot` based on new roots
    pub fn set_root(&mut self, new_root: Slot) {
        self.votes_per_slot = self.votes_per_slot.split_off(&new_root);
        // `self.votes_per_slot` now only contains entries >= `new_root`
    }

    pub fn remove_slot_hash(&mut self, slot: Slot, hash: &Hash) -> Option<Vec<Pubkey>> {
        self.votes_per_slot.get_mut(&slot).and_then(|slot_hashes| {
            slot_hashes.remove(hash)
            // If `slot_hashes` becomes empty, it'll be removed by `set_root()` later
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unfrozen_gossip_verified_vote_hashes_add_vote() {
        let mut unfrozen_gossip_verified_vote_hashes = UnfrozenGossipVerifiedVoteHashes::default();
        let mut latest_validator_votes_for_frozen_banks =
            LatestValidatorVotesForFrozenBanks::default();
        let num_validators = 10;
        let validator_keys: Vec<Pubkey> = std::iter::repeat_with(Pubkey::new_unique)
            .take(num_validators)
            .collect();

        // Case 1: Frozen banks shouldn't be added
        let frozen_vote_slot = 1;
        let num_repeated_iterations = 10;
        for _ in 0..num_repeated_iterations {
            let hash = Hash::new_unique();
            let is_frozen = true;
            for vote_pubkey in validator_keys.iter() {
                unfrozen_gossip_verified_vote_hashes.add_vote(
                    *vote_pubkey,
                    frozen_vote_slot,
                    hash,
                    is_frozen,
                    &mut latest_validator_votes_for_frozen_banks,
                );
            }

            assert!(unfrozen_gossip_verified_vote_hashes
                .votes_per_slot
                .is_empty());
        }

        // Case 2: Other >= non-frozen banks should be added in case they're frozen later
        for unfrozen_vote_slot in &[frozen_vote_slot - 1, frozen_vote_slot, frozen_vote_slot + 1] {
            // If the vote slot is smaller than the latest known frozen `vote_slot`
            // for each pubkey (which was added above), then they shouldn't be added
            let num_duplicate_hashes = 10;
            for _ in 0..num_duplicate_hashes {
                let hash = Hash::new_unique();
                let is_frozen = false;
                for vote_pubkey in validator_keys.iter() {
                    unfrozen_gossip_verified_vote_hashes.add_vote(
                        *vote_pubkey,
                        *unfrozen_vote_slot,
                        hash,
                        is_frozen,
                        &mut latest_validator_votes_for_frozen_banks,
                    );
                }
            }
            if *unfrozen_vote_slot >= frozen_vote_slot {
                let vote_hashes_map = unfrozen_gossip_verified_vote_hashes
                    .votes_per_slot
                    .get(unfrozen_vote_slot)
                    .unwrap();
                assert_eq!(vote_hashes_map.len(), num_duplicate_hashes);
                for pubkey_votes in vote_hashes_map.values() {
                    assert_eq!(*pubkey_votes, validator_keys);
                }
            } else {
                assert!(unfrozen_gossip_verified_vote_hashes
                    .votes_per_slot
                    .is_empty());
            }
        }
    }
}
