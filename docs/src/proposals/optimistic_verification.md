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
    * b) Claim of a `volated_optimistic_slot` (`84` in this case), and an `illegal_slot > violated_optimistic_slot` on a different fork which caused the violation (`88` in this case), and the hashes of both.
    * a) All the blocks from some snapshot root to the to the block `X` that contains the votes optimistically confirming `volated_optimistic_slot`.
    * b) All the blocks from the snapshot root to the the block `X'` that contains the first `> 2/3` votes on the fork containing `illegal_slot` (i.e. `X'` confirmed `illegal_slot`).

* 2) Other validators that are notified of this violation must verify:
    * a) The snapshot root is valid
    * b) Replay the provided `Proof of Violation` and verify a violation actually occurred. While replaying, record the hash of the `violated_optimistic_slot_hash`. 
    * c) The validator checks if they have some *earliest* `switching proof` for a slot `S` on the same fork as the slot with hash `voted_optimistic_slot_hash` (i.e. they switched away from that optimistically confirmed fork). where `(S == voted_optimistic_slot || descendants(voted_optimistic_slot).contains(S))` and `S < illegal_slot` (Essentially find the proofs for the first time validators switched away from the fork containing `voted_optimistic_slot`, before the violation occurred). If so, upload the proof.

# Switch Proof Contents
Each validator's switching proof contains:
* a) A starting `root` snapshot
* b) A `switching vote` they claim is the *earliest* vote they made on a slot `>84` onto fork `88` (so either `87` or `88` in this case)
* c) All the other votes on other forks they claim allowed them to make this switching vote. These votes in the proof are of the form
`(vote_slot, vote_pubkey, vote_observed_slot, num_confirmations)`, where `vote_pubkey` made a vote for slot `vote_slot`, and
`vote_observed_slot` is the slot at which `vote_slot` was observed to have confirmations == num_confirmations.
This vote implies the vaildator with vote pubkey == `vote_pubkey` was locked out at slot 84, i.e. `vote_slot + 2^num_confirmations >=84`.
* d) To verify ancestor/descendant relationships, the proof will also need to provide:
     * i) All the blocks from the `root` snapshot to `vote_observed_slot` for each vote included in the proof.
     * ii) All the blocks from the `root` snapshot to the slot that violated optimistic confirmation, slot, `84` in this case.

# Switch Proof Verification
Verification (Valid Switching Proof): Wait for all the validators to upload their proofs in some window of time on the order of an epoch.

* 1) Iterate through all the provided proofs, and find the one with the lowest snapshot root `snapshot_lowest`. From above, we know this proof includes all the ancestors, `a` of the violated optimistic slot `84`, in the range `snapshot_lowest <= a < 84`. Add all
these ancestors to a set called `optimistic_slot_ancestors`.

* 2)
Then we go through each of the provided validator's proofs.
    ```
        let mut possible_lockout_violaters = HashSet::new();
        
        // `switching_slots` are all the slots people claim were their earliest
        let earliest_switching_slots: HashMap<Pubkey, Slot>

        // Somebody may have lied that their proof was the *earliest*
        // switching vote, i.e. they made a switching vote earlier than the
        // switching slot they presented in their proof. To detect this we keep
        // track of their `proclaimed`

        for proof in proofs {
            for (vote_slot, vote_pubkey, vote_observed_slot, num_confirmations) in proof {
                possible_lockout_violaters.insert(vote_pubkey);

                // 1) Check validator doesn't include a vote from himself in proof (self-incriminating)

                // 2) Replay all blocks between `vote` and `vote_observed_slot` until
                // the number of confirmations == `num_confirmations`

                // 3) Check `vote_slot` is not a descendant or ancestor of `84`

                // 4) Check that the switching proof 
            }
        }
    ```

* 3) Verification (Lockouts): From step 2, if everybody's proof is valid, then 
somebody else must have violated lockout (everyone who generated a proof wasn't lying).


From step 2 we have replayed all the forks any validator included in a switching proof,
and a list of possible malicious actors in `possible_lockout_violaters`. So:

```
    // `forks` is all the banks at the tip of each fork excluding any forks descended
    // from `84`
    let forks;

    // `ancestors` are all the ancestors of slot `84`
    let ancestors: HashSet<Slot>;

    for fork_tip in forks {
        for maybe_bad_validator in possible_lockout_violaters {
            // Deserialize vote state for `maybe_bad_validator`

            // Get lockouts for `maybe_bad_validator`

            for (lockout_slot, num_confirmations) in lockouts {
                if !ancestors.contains(lockout_slot) && lockout_slot + 2^(num_confirmations) > 84 {
                    // Lockout violation detected!
                    return maybe_bad_validator;
                }
            }
        }
    }
```

# Examples of some malicious cases

