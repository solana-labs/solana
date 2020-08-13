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
Each validator's switching proof looks like:

`
    // A vote made by validator `V` on another forks that will
    // be referenced in a swiitching proof
    pub struct ValidatorLockoutProof {
        // Slot voted on by `V`
        pub locked_slot: Slot,
        // Hash of bank `locked_slot`
        pub locked_slot_hash: Hash,
        // Number of confirmations on `locked_slot`
        pub confirmation_count: u32,
        // The bank in which `confirmation_count` confirmations
        // was observed on `locked_slot`
        pub observed_bank_slot: Slot,
        pub observed_bank_hash: Hash,
    }

    pub struct SwitchProof {
        // Last vote slot on old fork before the switch
        pub last_vote_slot: Slot,
        // New vote slot on new fork being switched to
        pub new_fork_slot: Slot,
        // Map from validators' vote pubkeys to information about that validator's
        // vote on other forks. The proving validator claims allowed these votes
        // allowed them to make this switching vote.
        pub validator_proofs: HashMap<Pubkey, ValidatorLockoutProof>,
        pub proof_stake: u64,
        pub total_stake: u64,
        // The `authorized_voter_pubkey` of the node at the epoch they made the
        // vote on `new_fork_slot`
        pub authorized_voter_pubkey: Pubkey,
        // The validator vote account
        pub vote_pubkey: Pubkey
    }

    pub struct FinalizedSwitchProof {
        pub data: Vec<u8>,
        // Hash of the switch proof in `data` and the previous SwitchProof
        pub hash: Hash,
        // Signature of `hash`, signed with the authorized voter keypair
        // for the epoch where the vote for the slot on the new fork 
        // was made
        pub signature: Signature,
        pub authorized_vote_pubkey: Pubkey,
    }
`

# Submmited Proof Contents
This is the structure of the proof that's read by the verifier:
`
    struct SubmittedProof {
        serialized_lowest_switch_proof: FinalizedSwitchProof,
        root_snapshot: Snapshot,
        blockstore: Blockstore,
        all_switch_proof_hashes: Vec<Hash>,
        potential_violators_vote_transactions: Vec<Transaction>,
    }

    pub struct FinalizedSubmittedProof{
        // Serialized `SubmittedProof`
        submitted_proof: Vec<u8>,
        vote_pubkey: Pubkey,
        // Signed by `vote_pubkey`, confirming a validator endorses this proof
        signature: Signature,
    }
`
* a) The serialized proof `serialized_lowest_switch_proof`, where the deserialized proof is the
proof the validator claims is the first switch they made after voting on `violated_optimistic_slot`.
Let's refer to this deserialized proof as `deserialized_proof`.
* b) The starting `root_snapshot` snapshot.
* c) `blockstore` with necessary blocks to verify ancestor/descendant relationships. These include:
     * i) All the blocks from the `root` snapshot to each slot where lockouts were observed in the proof, i.e. all the slots in `deserialized_proof.validator_proofs.values().map(|validator_lockout_proof| validator_lockout_proof.observed_bank_slot)`.
     * ii) All the blocks from the `root` snapshot to the block that shows optimistic confirmation was achieved, i.e. the block that violated optimistic confirmation, slot, `84` in this case.
* d) The `potential_violators_vote_transactions` set, which is vote transactions from `>2/3` staked validators voting for `violated_optimistic_slot` as the last slot in the transaction vote stack, proving the slot was optimistically confirmed. (Redundant with the `Proof of Violation` as these are already included there, but necessary until `Proof of Violation`'s are implemented). *Note* this means validators must now record the set of validators in gossip who voted on a slot before reporting optimistic confirmation to clients.
* e) `all_switch_proof_hashes` - Hash of all switching proofs this validator has made > `violated_optimistic_slot`. This is important to show proofs were actually made in the declared order, because each switching proof hash is the hash of `(contents
of the proof, previous_switching_proof_hash)` (see description of `FinalizedSwitchProof.hash` above).


# Switch Proof Verification
Verification (Valid Switching Proof): Wait for all the validators to upload their proofs in some window of time on the order of an epoch. 

Given a set of `FinalizedSubmittedProof`'s, our goal is to find the set of invalid proofs, `invalid_proofs`.

* 1) Verify the signatures of and deserialize all the `FinalizedSubmittedProof`. We should end up with a
mapping from vote pubkey to submitted proof, of the form `submitted_proofs: HashMap<Pubkey, SubmittedProof>`.

* 2) TODO: First we need to agree on a trusted snapshot root. For now we just use the lowest one provided
in any of the `SubmittedProof`s. Load the bank form the trusted snapshot, we call this the `trusted_bank`.

* 3) Iterate through each `proof` in `submitted_proofs`, deserializing each `proof.serialized_lowest_switch_proof`, verifying the signature of the proofs were made by the correct authorized voter for that vote account in that epoch (TODO: Requires trusting the snapshot), and returning a map from authorized voters to their switching proofs, `switch_proofs: HashMap<Pubkey, CandidateProof>` as follows:

```
struct CandidateProof {
    submitted_proof: SubmittedProof,
    switch_proof: SwitchProof,
    switch_proof_hash: Hash,
}

fn deserialize_switch_proofs(
    // The optimistic slot that was rolled back
    violated_optimistic_slot: Slot,
    // Map from vote pubkey to submitted proof
    submitted_proofs: HashMap<Pubkey, SubmittedProof>,
    trusted_bank: &Bank,
    invalid_proofs: &mut HashSet<Pubkey>,
) -> HashMap<Pubkey, (SwitchProof, Hash)> {
    submitted_proofs.into_iter().filter_map(|(vote_pubkey, submitted_proof)| {
        let finalized_switch_proof: FinalizedSwitchProof = &submitted_proof.serialized_lowest_switch_proof;
        let switch_proof: SwitchProof = deserialize(&finalized_switch_proof.data);
        
        if switch_proof.last_vote_slot < violated_optimistic_slot || 
        switch_proof.new_fork_slot <= violated_optimistic_slot 
        {
            invalid_proofs.insert(vote_pubkey);
            return None;
        }

        // Get the epoch in which the switch vote was made
        let violated_optimistic_slot_epoch = bank.epoch_schedule().get_epoch(switch_proof.new_fork_slot);

        // Check the authorized voter is the correct one for the switch epoch
        // Get authorized voter for the epoch == `violated_optimistic_slot_epoch`
        // This information should exist in `trusted_bank` as long as the `trusted_bank`
        // is not more than an epoch behind `violated_optimistic_slot`, which should be
        // ok as long as validators have been snapshotting regularly (TODO: verify 
        // if this is safe assumption).
        let authorized_voter_pubkey = 
                trusted_bank
                .epoch_stakes
                .get(violated_optimistic_slot_epoch)
                .expect("Epoch stakes for bank's own epoch must exist")
                .epoch_authorized_voters()
                .get(vote_pubkey)

        if authorized_voter_pubkey != finalized_switch_proof.authorized_vote_pubkey {
            invalid_proofs.insert(vote_pubkey);
            return None;
        }

        // Verify the switching proof hash is equal to the hash of the previous switching
        // proof and thee proof data
        let let prev_hash = submitted_proof.all_switch_proof_hashes.get_prev_hash(finalized_switch_proof.hash);
        if hash((prev_hash, finalized_switch_proof.data)) != finalized_switch_proof.hash {
            invalid_proofs.insert(vote_pubkey);
            return None;
        }

        // Verify the switching proof signature is a signature of the switch proof hash,
        // made by the authorized voter
        if !verify_signature(finalized_switch_proof.hash, finalized_switch_proof.signature, authorized_voter_pubkey) {
            invalid_proofs.insert(vote_pubkey);
            return None;
        }

        Some((vote_pubkey, CandidateProof {
            submitted_proof,
            switch_proof, 
            switch_proof_hash: finalized_switch_proof.hash
        }))
    }).collect()
}
```

* 4) Iterate through each `proof` in `SubmittedProofs`, parse the `proof.potential_violators_vote_transactions` until we find a `slashing_candidate_vote_keys` set that is valid (passes signature checks). These are the validators who voted on `violated_optimistic_slot`, so some of them must be slashable. Invalid proofs are added to `invalid_proofs`.

The procedure is as follows:

```
fn slashing_candidate_vote_keys(
    violated_optimistic_slot: Slot,
    // Bank hash of the `violated_optimistic_slot`. Should be given by the violation
    // detection mechanism
    violated_optimistic_hash: Hash,
    // Validator that submmitted this proof
    vote_pubkey: &Pubkey,
    submitted_proof: SubmittedProof,
    invalid_proofs: &mut HashSet<Pubkey>,
    trusted_bank: &Bank,
) -> Option<HashSet<Pubkey>> {
    // Parse the set of optimistic votes made by the potential slashing candidates
    let potential_violators_vote_transactions = submitted_proof.potential_violators_vote_transactions;

    // Load the snapshot
    let violated_optimistic_slot_epoch = trusted_bank.epoch_schedule().get_epoch(violated_optimistic_slot);

    // Verify the vote transactions
    let mut slashing_candidate_vote_keys = HashSet::new();
    for vote_transaction in potential_violators_vote_transactions {
        let vote = vote_from_tx(&vote_transaction);

        // Check the vote is for the right slot and bank hash
        if !vote.slots.last() == violated_optimistic_slot_epoch || vote.hash != violated_optimistic_hash {
            invalid_proofs.insert(vote_pubkey);
            return None;
        }

        // Get authorized voter for the epoch == `violated_optimistic_slot_epoch`
        // This information should exist in `trusted_bank` as long as the `trusted_bank`
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
            invalid_proofs.insert(vote_pubkey);
            return None;
        }

        slashing_candidate_vote_keys.insert(vote_pubkey);
    }

    Some(slashing_candidate_vote_keys)
}
```

* 5) Next, we take each key in `slashing_candidate_vote_keys` and iterate through
their deserialized `candidate_proofs` to build  `slashing_candidate_proofs`, which maps
each of the potential slashing validators to their `CandidateProof`.

```
fn slashing_candidate_proofs(
    // All the uploaded proofs
    candidate_proofs: &HashMap<Pubkey, CandidateProof>,
    slashing_candidate_vote_keys: &HashSet<Pubkey>,
    violated_optimistic_slot: Slot,
    invalid_proofs: &mut HashSet<Pubkey>,
    missing_proofs: &mut HashSet<Pubkey>,
) -> HashMap<Pubkey, Option<&CandidateProof>> {
    slashing_candidate_vote_keys.map
    (|vote_pubkey| {
        let validator_proofs = candidate_proofs.get(vote_pubkey);
        if validator_proofs.is_none() {
            missing_proofs.insert(vote_pubkey);
            return None;
        }

        let proofs = validator_proofs.unwrap();

        // The switching vote must occur after the optimistic slot
        if proofs.switch_proof.new_fork_slot < violated_optimistic_slot {
            invalid_proofs.insert(vote_pubkey);
            return None;
        }

        Some(proofs)
    }).collect()
}
```

* 6) We now start the heart of the verification process as follows:
```
    fn verify_proofs {
        violated_optimistic_slot: Slot,
        violated_optimistic_slot_hash: Hash;
        // Built in step 5
        slashing_candidate_proofs: &HashMap<Pubkey, Option<&CandidateProof>>,
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
                slashing_candidate_proofs,
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
        slashing_candidate_proofs: &HashMap<Pubkey, Option<&CandidateProof>>,
        entry_proof: &SwitchProof,
        invalid_proofs: &mut HashSet<Pubkey>,
        missing_proofs: &mut HashSet<Pubkey>,
        violated_lockouts: &mut HashSet<Pubkey>,
        replayed_proof_keys: &mut HashSet<Pubkey>,
    ){
        let mut unexplored_proof_keys: Vec<Pubkey> = vec![entry_proof];

        while !unexplored_proof_keys.is_empty() {
            let prover_vote_pubkey = unexplored_proof_keys.pop().unwrap();
            assert!(!replayed_proof_keys.contains(&prover_vote_pubkey));
            assert!(!violated_lockouts.contains(&prover_vote_pubkey));
            assert!(!missing_proofs.contains(&prover_vote_pubkey));

            let proofs = slashing_candidate_proofs.get(&prover_vote_pubkey);

            // If this proof wasn't submitted by a slashing candidate, we don't care
            if proofs.is_none() {
                continue;
            }

            replayed_proof_keys.insert(prover_vote_pubkey);

            // This person will be slashed for not submitting a proof
            if proofs.unwrap().is_none() {
                assert!(missing_proofs.contains(&proof_key));
                continue;
            }

            // Replay the blockstore from the snapshot and blockstore in `proofs.submitted_proof`
            let proofs = proofs.unwrap();
            let bank_forks = new_banks_from_ledger(proofs.submitted_proof);
            let violated_optimistic_slot_bank = bank_forks.get(&violated_optimistic_slot);

            // Proof must provide all blocks from snapshot root to the correct 
            // version of violated_optimistic_slot so that verifier can verify chaining
            // relative to votes in the proof
            if violated_optimistic_slot_bank.is_none() {
                invalid_proofs.insert(prover_vote_pubkey);
                continue;
            }
            let violated_optimistic_slot_bank = let violated_optimistic_slot_bank.unwrap();
            if violated_optimistic_slot_bank.bank_hash() != violated_optimistic_slot_hash {
                invalid_proofs.insert(prover_vote_pubkey);
                continue;
            }

            // Iterate through the votes this validator claims were locked
            // out past the `violated_optimistic_slot` when they switched
            let ancestors = bank_forks.ancestors();
            let total_proof_vote_stake = 0;
            for (lockout_proof_vote_pubkey, ValidatorLockoutProof {
                locked_slot,
                // The slot at which which `confirmation_count` confirmations
                // was observed on `locked_slot`
                observed_bank_slot,
                ..
            }) in proofs.switch_proof.validator_proofs {
                // If this pubkey isn't in the potentially slashable set, ignore it
                if !slashing_candidate_proofs.contains_key(lockout_proof_vote_pubkey) {
                    continue;
                }

                // Verify the slot is not an ancestor or descendant of `violated_optimistic_slot`
                if ancestors.get(observed_bank_slot).contains(violated_optimistic_slot) {
                    // Proof is invalid, no need to check the rest of it
                    invalid_proofs.insert(prover_vote_pubkey);
                    break;
                }
                if ancestors.get(violated_optimistic_slot).contains(observed_bank_slot) {
                    // Proof is invalid, no need to check the rest of it
                    invalid_proofs.insert(prover_vote_pubkey);
                    break;
                }

                // Verify the lockout is past `violated_optimistic_slot`
                let vote_observed_bank = bank_forks.get(&observed_bank_slot);
                if vote_observed_bank.map(|vote_observed_bank|
                    vote_observed_bank.get_vote_state(lockout_proof_vote_pubkey)
                ).and_then(|vote_state|
                    vote_state.get_lockout(locked_slot)
                ).and_then(|lockout|
                    if lockout >= violated_optimistic_slot {
                        Some(()))
                    } else {
                        None
                    }
                ).is_none() {
                    // Proof is invalid, no need to check the rest of it
                    invalid_proofs.insert(prover_vote_pubkey);
                    break;
                }

                // Now to check for lockout violation, let's review what we know:
                // 1) The `lockout_proof_vote_pubkey` is in the `slashing_candidate_proofs` map,
                // which means validator with vote account `lockout_proof_vote_pubkey` must have voted on
                // `violated_optimistic_slot`. 
                //
                // 2) The validator is locked out on `locked_slot` past `violated_optimistic_slot`, and 
                // `locked_slot` is not on the same fork as `violated_optimistic_slot`.
                //
                // 3) This means it must be `locked_slot > violated_optimistic_slot`, otherwise,
                // the validator with vote account `lockout_proof_vote_pubkey` must have violated lockout.
                if locked_slot <= violated_optimistic_slot {
                    replayed_proof_keys.insert(lockout_proof_vote_pubkey);
                    violated_lockouts.insert(lockout_proof_vote_pubkey);
                }

                // Otherwise this vote passed all the checks, so we check if we've already played a proof from the validator with vote account == `vote_pubkey`.
                if replayed_proofs.contains_key(lockout_proof_vote_pubkey) {
                    // This voter was in `slashing_candidate_proofs` map, so if they're
                    // not in the `missing_proofs` set they should exist here
                    if !missing_proofs.contains(lockout_proof_vote_pubkey) {
                        // The earliest switch slot, as claimed by this validator's uploaded proof
                        let lockout_voter_proofs = slashing_candidate_proofs.get(lockout_proof_vote_pubkey).unwrap().unwrap();

                        let (lockout_voter_proof_switch_slot, lockout_voter_proof_hash)

                        // Get the vote transaction for this slot from the bank, and look at the
                        // `last_switch_slot`
                        let (vote_referenced_switch_slot, vote_referenced_switch_hash) = 
                        vote_observed_bank.get_vote_tx_for_slot(slot).last_switch_slot;

                        if lockout_voter_proofs.switch_proof.new_fork_slot > vote_referenced_switch_slot {
                            // We found a vote from validator with vote pubkey `lockout_proof_vote_pubkey`
                            // in another proof that was earlier than the proclaimed
                            // earliest vote in their own proof, which is a slashable violation.
                            invalid_proofs.insert(lockout_proof_vote_pubkey);
                            continue;
                        }

                        // Check that there's a valid hash chain between the two switching
                        // proofs with hashes `lockout_voter_proof_hash` and `vote_referenced_switch_hash`
                        
                        // If the hashes are valid, this shows the "lowest" switching
                        // proof provided by the validator with key `lockout_proof_vote_pubkey` was 
                        // actually made before the `vote` referenced in this other validator's proof.

                        // If the hashes are not valid, that means the switch proof hash that 
                        // validator `lockout_proof_vote_pubkey` submitted in their proof could not
                        // be shown to be earlier than the switch hash in this vote (this vote was
                        // replayed so it must have been signed by them!). This means the validator
                        // must have made up the switch proof hash in their proof, which is a slashable
                        // offense
                        if !lockout_voter_proofs.submitted_proofs
                            .all_switch_proof_hashes
                            .is_hash_chain(
                                lockout_voter_proofs.switch_proof_hash,          vote_referenced_switch_hash) 
                        {
                            invalid_proofs.insert(lockout_proof_vote_pubkey);
                            break;
                        }
                    }
                } else if !violated_lockouts.contains(lockout_proof_vote_pubkey) && !missing_proofs.contains(lockout_proof_vote_pubkey) {
                    // Add validator with vote account == `vote_pubkey` to the list of proofs to check.
                    unexplored_proofs.push(lockout_proof_vote_pubkey);
                }

                // Add the vote stake
                total_proof_vote_stake += violated_optimistic_slot_bank.epoch_vote_account_stake(lockout_proof_vote_pubkey);
            }

            // Check the vote stakes added up to more than the threshold
            if total_proof_vote_stake / violated_optimistic_slot_bank.total_epoch_stake() < VOTE_SWITCH_THRESHOLD {
                invalid_proofs.insert(prover_vote_pubkey);
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