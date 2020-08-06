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

1) We detect optimistic confirmation on `84` has been violated by the root on `88`. Root on `88` means we have seen at least `>2/3` vote on descendant of `88`, so this is a conflict.

2) There must be `>1/3` overlapping between each the `>2/3` groups who voted on `84` and `88`. We ask these `>1/3` people to upload their proofs. It is important to note here that that the bad actor may not be in this `>1/3` overlap group. For instance somebody else outside this group who voted on `84` may have violated their lockout by voting on say `79`, allowing others to generate a valid proof, but the bad actor himself did not vote on `88`. Our verification mechanism should catch such a bad actor as well.

3) Each validator's proof is:
       a) a starting `root` snapshot
       b) a `switching vote` they claim is the earliest vote they made on a slot `>84` onto fork `88` (so either `87` or `88` in this case)
       c) All the other votes on other forks they claim allowed them to make this switching vote. These votes in the proof are of the form
       `(vote_slot, vote_pubkey, vote_observed_slot, num_confirmations)`, where `vote_pubkey` made a vote for slot `vote_slot`, and
       `vote_observed_slot` is the slot at which `vote_slot` was observed to have confirmations == num_confirmations.
       This vote implies the vaildator with vote pubkey == `vote_pubkey` was locked out at slot 84, i.e. `vote_slot + 2^num_confirmations >=84`.

       To verify these, the verifier will potentially need all the blocks from the `root` snapshot to `vote_observed_slot`.

4) Verification (Valid Switching Proof): First we replay each of the two forks up to slot 84 and slot 88.
Then we go through each of the `>1/3` validator's proof.

    ```
        let mut possible_lockout_violaters = HashSet::new();
        for proof in proofs {
            for (vote_slot, vote_pubkey, vote_observed_slot, num_confirmations) in proof {
                possible_lockout_violaters.insert(vote_pubkey);

                // 1) Check validator doesn't include a vote from himself in proof (self-incriminating)

                // 2) Replay all blocks between `vote` and `vote_observed_slot` until
                // the number of confirmations == `num_confirmations`

                // 3) Check `vote_slot` is not a descendant or ancestor of `84`
            }
        }
    ```

5) Verification (Lockouts): From step 4, if everybody's vote is valid, then that means somebody
else must have violated lockout (everyone who generated a proof wasn't lying).

From step 4 we have replayed all the forks any validator included in a switching proof,
and a list of possible malicious actors in `possible_lockout_violaters`. So:

```
    // `forks` is all the banks at the tip of each fork excluding any forks descended
    // from `84`
    let forks;

    // ancestors are all the ancestors of slot `84`
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