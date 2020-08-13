---
title: Detection and Verification of Optimistic Rollback
---

# Prooblem 

Imagine a scenario:
```
          79 -  77 - 76
                      |
     78  ------- |    |
                 |    |
88(rooted)--87---75 -74
                      |------70
84 (>2/3 optimistic)---
```

Slot `84` has been optimistically confirmed, but this block is then rolledd backby the root on `88`. We need a mechanism to prove such a violation occurred and then subsequently find who was responsible for the violation.

There must be `>1/3` overlapping between each the `>2/3` groups who voted on `84` and `88`. We ask these `>1/3` people to upload their proofs. It is important to note here that that the bad actor may not be in this `>1/3` overlap group. For instance somebody else outside this group who voted on `84` may have violated their lockout by voting on say `79`, allowing others to generate a valid proof, but the bad actor himself did not vote on `88`. Our verification mechanism should catch such a bad actor as well.

# Violation Detection
We detect optimistic confirmation on `84` has been violated by the root on `88`. Root on `88` means we have seen at least `>2/3` vote on descendant(s) of `88`, so this is a conflict. 

* 1) The validator(s) that catch this violation should upload a proof of the violation by uploading to the verification server a `Proof of Violation`:
    * a) A snapshot root
    * b) Claim of a `violated_optimistic_slot` (`84` in this case), and an `illegal_slot > violated_optimistic_slot` on a different fork which caused the violation (`88` in this case), and the hashes of both.
    * c) Vote transactions from `>2/3` staked validators voting for `violated_optimistic_slot` as the last slot in the transaction vote stack, proving the slot was optimistically confirmed.
    * d) All the blocks from the snapshot root to the the block `X'` that contains the first `> 2/3` votes on the fork containing `illegal_slot` (i.e. `X'` confirmed `illegal_slot`).

* 2) Other validators that are notified of this violation must verify:
    * a) The snapshot root is valid
    * b) Replay the provided `Proof of Violation` and verify a violation actually occurred. While replaying, record the hash of the `violated_optimistic_slot_hash`. 
    * c) The validator checks if they have some *earliest* `switching proof` for a slot `S` on the same fork as the slot with hash `voted_optimistic_slot_hash` (i.e. they switched away from that optimistically confirmed fork). where `(S == voted_optimistic_slot || descendants(voted_optimistic_slot).contains(S))` and `S < illegal_slot` (Essentially find the proofs for the first time validators switched away from the fork containing `voted_optimistic_slot`, before the violation occurred). If so, upload the proof.

# Switch Proof Contents
Each validator's switching proof contains:
* a) A starting `root_snapshot` snapshot
* b) A `switching_vote` they claim is the *earliest* vote they made on a slot `>84` onto any other fork (so either `87` or `88` in this case).
* c) The `authorized_voter_pubkey` of the node at the epoch they made the `switching_vote`
* d) A list of `proof_votes`, which are all the other votes on other forks this validator claims allowed them to make this switching vote. These votes in the proof are of the form
`(vote_slot, vote_pubkey, vote_observed_slot, num_confirmations)`, where `vote_pubkey` made a vote for slot `vote_slot`, and
`vote_observed_slot` is the slot at which `vote_slot` was observed to have confirmations == num_confirmations.
This vote implies the vaildator with vote pubkey == `vote_pubkey` was locked out at slot 84, i.e. `vote_slot + 2^num_confirmations >=84`.
* e) To verify ancestor/descendant relationships, the proof will also need to contain some blocks:
     * i) All the blocks from the `root` snapshot to `vote_observed_slot` for each vote included in the proof.
     * ii) All the blocks from the `root` snapshot to the block that shows optimistic confirmation was achieved, i.e. the block that violated optimistic confirmation, slot, `84` in this case.
* f) The `potential_violators_vote_transactions` set, which is vote transactions from `>2/3` staked validators voting for `violated_optimistic_slot` as the last slot in the transaction vote stack, proving the slot was optimistically confirmed. (Redundant with the `Proof of Violation` as these are already included there, but necessary until `Proof of Violation`'s are implemented).

# Switch Proof Verification
Verification (Valid Switching Proof): Wait for all the validators to upload their proofs in some window of time on the order of an epoch.

Our goal is to find the set of `invalid_proofs`.

* 1) First, we iterate through each `proof` in all the proofs, parsing the `proof.potential_violators_vote_transactions` set until we get to the first valid proof that successfully builds a `potential_violators_authorized_voting_keys`. Invalid proofs are added to `invalid_proofs`.

The procedure is as follows:

```
fn potential_violators_authorized_voting_keys(
    // Bank hash of the `violated_optimistic_slot`. Should be given by the violation
    // detection mechanism
    violated_optimistic_hash: Hash,
    proof: SwitchProof,
    invalid_proofs: &mut HashSet<Pubkey>,
) -> Option<HashSet<Pubkey>> {
    // Parse the proof
    let potential_violators_vote_transactions = proof.potential_violators_vote_transactions;
    let violated_optimistic_slot = proof.violated_optimistic_slot;

    // Load the snapshot
    let root_bank = load_bank_from_snapshot(proof.root_snapshot);
    let violated_optimistic_slot_epoch = bank.epoch_schedule().get_epoch(violated_optimistic_slot);

    // Verify the vote transactions
    let mut potential_violators_authorized_voting_keys = HashSet::new();
    for vote_transaction in potential_violators_vote_transactions {
        let vote = vote_from_tx(&vote_transaction);

        // Check the vote is for the right slot and bank hash
        if !vote.slots.last() == violated_optimistic_slot_epoch || vote.hash != violated_optimistic_hash {
            invalid_proofs.insert(proof.authorized_voter_pubkey);
            return None;
        }

        // Get authorized voter for the epoch == `violated_optimistic_slot_epoch`
        // This information should exist in `root_bank` as long as the `root_bank`
        // is not more than an epoch behind `violated_optimistic_slot`, which should be
        // ok as long as validators have been snapshotting regularly (TODO: verify 
        // if this is safe assumption).
        let authorized_voter_pubkey = 
                root_bank
                .epoch_stakes
                .get(violated_optimistic_slot_epoch)
                .expect("Epoch stakes for bank's own epoch must exist")
                .epoch_authorized_voters()
                .get(vote_account)

        // Verify signature
        if !verify_signature(vote_transaction, authorized_voter_pubkey) {
            invalid_proofs.insert(proof.authorized_voter_pubkey);
            return None;
        }

        potential_violators_authorized_voting_keys.insert(authorizeed_voter_pubkey);
    }

    Some(potential_violators_authorized_voting_keys)
}
```

* 2) Next, we take each key in `potential_violators_authorized_voting_keys` and iterate through
their uploaded proofs to build  `lowest_switch_votes` mapping each of the validators to the `switching_vote` in their proof.
```
fn lowest_switch_votes(
    // All the uploaded proofs
    all_proofs: &HashMap<Pubkey, SwitchProof>,
    potential_violators_authorized_voting_keys: &HashSet<Pubkey>,
    violated_optimistic_slot: Slot,
    invalid_proofs: &mut HashSet<Pubkey>,
    missing_proofs: &mut HashSet<Pubkey>,
) -> HashMap<Pubkey, Slot> {
    let switch_votes: HashMap<Pubkey, Slot> = potential_violators_authorized_voting_keys.filter_map
    (|pubkey| {
        let validator_proof = all_proofs.get(pubkey);
        if validator_proof.is_none() {
            missing_proofs.insert(pubkey);
            return None;
        }

        let validator_proof = validator_proof.unwrap();
        let switching_vote = validator_proof.switching_vote;

        // The switching vote must occur after the optimistic slot
        if switching_vote < violated_optimistic_slot {
            invalid_proofs.insert(validator_proof.authorized_voter_pubkey);
            return None;
        }

        Some((pubkey, switching_vote))
    }).collect();

    switch_votes
}
```

* 3) We now start the heart of the verification process as follows:
```
    fn verify_proofs {
        violated_optimistic_slot: Slot,
        violated_optimistic_slot_hash: Hash;
        // All the uploaded proofs
        all_proofs: &HashMap<Pubkey, SwitchProof>,
        // Built in step 1
        potential_violators_authorized_voting_keys: &HashSet<Pubkey>,
        // Build in step 2
        lowest_switch_votes: &HashMap<Pubkey, Slot>,
        invalid_proofs: &mut HashSet<Pubkey>,
        missing_proofs: &mut HashSet<Pubkey>,
        violated_lockouts: &mut HashSet<Pubkey>,
    } {
        let mut replayed_proof_keys: HashSet<Pubkey> = HashSet::new();

        while !replayed_proof_keys.len() == all_proofs.len() {
            let entry_proof = all_proofs.iter().find_map(|(k, v)| {
                if !replayed_proof_keys.contains(k) {
                    Some(v)
                } else {
                    None
                }
            }).unwrap();
            verify_connected_proofs(
                violated_optimistic_slot,
                violated_optimistic_slot_hash,
                all_proofs,
                potential_violators_authorized_voting_keys,
                lowest_switch_votes,
                entry_proof,
                invalid_proofs,
                missing_proofs,
                violated_lockouts,
                &mut replayed_proof_keys,
            )
        }
    }

    // Verify all the proofs referenced/reachable from `entry_proof`
    fn verify_connected_proofs(
        violated_optimistic_slot: Slot,
        violated_optimistic_slot_hash: Hash;
        all_proofs: &HashMap<Pubkey, SwitchProof>,
        potential_violators_authorized_voting_keys: &HashSet<Pubkey>,
        lowest_switch_votes: &HashMap<Pubkey, Slot>,
        entry_proof: &SwitchProof,
        invalid_proofs: &mut HashSet<Pubkey>,
        missing_proofs: &mut HashSet<Pubkey>,
        violated_lockouts: &mut HashSet<Pubkey>,
        replayed_proof_keys: &mut HashSet<Pubkey>,
    ){
        let mut unexplored_proof_keys: Vec<Pubkey> = vec![entry_proof];

        while !unexplored_proof_keys.is_empty() {
            let proof_key = unexplored_proof_keys.pop().unwrap();
            assert!(!replayed_proof_keys.contains(&proof_key));
            assert!(!violated_lockouts.contains(&proof_key));
            assert!(!missing_proofs.contains(&proof_key));

            let proof = all_proofs.get(&proof_key);
            replayed_proof_keys.insert(proof.authorized_voter_pubkey);
            if proof.is_none() {
                missing_proofs.insert(&proof_key);
                continue;
            }

            // Replay all the blocks in the proof
            let bank_forks = new_banks_from_ledger(proof);
            let violated_optimistic_slot_bank = bank_forks.get(&violated_optimistic_slot);

            // Proof must provide all blocks from snapshot root to the correct 
            // version of violated_optimistic_slot so that verifier can verify chaining
            // relative to votes in the proof
            if violated_optimistic_slot_bank.is_none() {
                invalid_proofs.insert(proof.authorized_voter_pubkey);
                continue;
            }
            let violated_optimistic_slot_bank = let violated_optimistic_slot_bank.unwrap();
            if violated_optimistic_slot_bank.bank_hash() != violated_optimistic_slot_hash {
                invalid_proofs.insert(proof.authorized_voter_pubkey);
                continue;
            }

            // Iterate through the votes this validator claims were locked
            // out past the `violated_optimistic_slot` when they switched
            let total_proof_vote_stake = 0;
            for (vote_slot, vote_pubkey, vote_observed_slot, num_confirmations) in proof.proof_votes {
                // If this pubkey isn't in the potentially slashable set, ignore it
                let vote_authorized_voter_pubkey = violated_optimistic_slot_bank.epoch_authorized_vote(vote_pubkey);
                if !potential_violators_authorized_voting_keys.contains(vote_authorized_voter_pubkey) {
                    continue;
                }

                // Verify the slot is not an ancestor or descendant of `violated_optimistic_slot`
                if ancestors.contains(violated_optimistic_slot) {
                    // Proof is invalid, no need to check the rest of it
                    invalid_proofs.insert(proof.authorized_voter_pubkey);
                    break;
                }
                if descendants.contains(violated_optimistic_slot) {
                    // Proof is invalid, no need to check the rest of it
                    invalid_proofs.insert(proof.authorized_voter_pubkey);
                    break;
                }

                // Verify the lockout is past `violated_optimistic_slot`
                let vote_observed_bank = bank_forks.get(&vote_observed_slot);
                if vote_observed_bank.map(|vote_observed_bank|
                    vote_observed_bank.get_vote_state(vote_pubkey)
                ).and_then(|vote_state|
                    vote_state.get_lockout(vote_slot)
                ).and_then(|lockout|
                    if lockout >= violated_optimistic_slot {
                        Some(()))
                    } else {
                        None
                    }
                ).is_none() {
                    // Proof is invalid, no need to check the rest of it
                    invalid_proofs.insert(proof.authorized_voter_pubkey;
                    break;
                }

                // Now to check for lockout violation, let's review what we know:
                // 1) The `vote_pubkey` is in the `potential_violators_authorized_voting_keys` set,
                // which means validator with vote account `vote_pubkey` must have voted on
                // `violated_optimistic_slot`. 
                //
                // 2) The validator is locked out on `vote_slot` past `violated_optimistic_slot`, and 
                // `vote_slot` is not on the same fork as `violated_optimistic_slot`.
                //
                // 3) This means it must be `vote_slot > violated_optimistic_slot`, otherwise,
                // the validator with vote account `vote_pubkey` must have violated lockout.
                if vote_slot <= violated_optimistic_slot {
                    replayed_proof_keys.insert(authorized_voter);
                    violated_lockouts.insert(authorized_voter);
                }

                // Otherwise this vote passed all the checks, so we check if we've already played a proof from the validator with vote account == `vote_pubkey`.
                if replayed_proofs.contains_key(vote_authorized_voter_pubkey) {
                    // This voter was in `potential_violators_authorized_voting_keys`, so if they're
                    // not in the `missing_proofs` set they should exist here
                    if !missing_proofs.contains(vote_authorized_voter_pubkey) {
                        // The earliest switch slot, as claimed by this validator's uploaded proof
                        let lowest_switch_slot = lowest_switch_votes.get(vote_authorized_voter_pubkey).unwrap();

                        // We found a vote this validator made that was earlier than they're proclaimed
                        // earliest vote, which is a slashable violation.
                        if lowest_switch_slot > vote_observed_bank.get_vote_tx_for_slot(slot).last_switch_slot {
                            invalid_proofs.insert(vote_authorized_voter_pubkey);
                        }
                    }
                } else if !violated_lockouts.contains(vote_authorized_voter_pubkey) && !missing_proofs.contains(vote_authorized_voter_pubkey) {
                    // Add validator with vote account == `vote_pubkey` to the list of proofs to check.
                    unexplored_proofs.push(vote_authorized_voter_pubkey);
                }

                // Add the vote stake
                total_proof_vote_stake += violated_optimistic_slot_bank.epoch_vote_account_stake(vote_pubkey);
            }

            // Check the vote stakes added up to more than the threshold
            if total_proof_vote_stake / violated_optimistic_slot_bank.total_epoch_stake() < VOTE_SWITCH_THRESHOLD {
                invalid_proofs.insert(proof.authorized_voter_pubkey);
            }
        }
    }
```

TODD: How to make sure the snapshots in the bank have verifiably correct state (i.e. things like mapping 
vote_pubkeys -> authorized voters, so that we know the authorized voter is real so people can't "fake" other people's vote transactions?)

# Examples of some tricky cases

## Fake proofs
```
    /-----2 (Validator A, Validator B)

   1 -------- 11 - 15 (Validator A)

    \------5 (Validator C) - 8 (Validator B) - 9 (Validator B)
```

1) `A` didn't generate a switching proof, voted on `11 -> 15` illegally. `B` generated a valid proof using `A's` vote for `11`
2) `B` didn't generate a switching proof, voted on `8->9` illegally. `A` generated a valid proof using `B's` vote for `9`






