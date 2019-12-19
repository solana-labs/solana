# Booting From a Snapshot

## Problem

A validator booting from a snapshot does not currently confirm that it is
caught up to the correct cluster before voting. Due to this, the validator may
get itself into a situation where it is required to wait before voting to avoid
slashing, where confirmation of the previous condition would have allowed it to
vote earlier. The two situations are:

### Locked Out of the Correct Cluster

The validator may be fed a snapshot and gossip info for a different (though not
necessarily malicious) cluster. As an example, assume that cluster is at slot
100,000 while the cluster the validator intended to join is at 10,000. If the
validator votes once on the wrong cluster, it must not vote on the correct
cluster for at least 90,000 slots to avoid being slashed. This is because
slashing is based on slot numbers, so when the validator makes a `vote` on the
wrong cluster's slot 100,000, it is locked out of voting on any slot < `100,000
+ lockout(vote)` on the correct cluster. Avoiding this situation requires the
correct cluster check.

### Voting Before Catching Up

Any vote that a validator signs must have its lockout observed. If a validator
has to reboot, the most secure way for it to check if its lockouts apply to the
new set of forks is to see the votes on the ledger. If it can find a vote on
the ledger, it knows that that vote does not lock it out of voting on the
current fork. For example, consider a validator that participates in a cluster
and lands votes for every slot up to and including 100, after which it reboots.
If it gets a snaphsot for slot 200, and it can see that all of its votes are in
its vote account for slot 200, it knows that 200 must be a descendent of 100,
so it can safely vote on 200 and its descendents. However, assume that the
cluster is at 300 when the validator gets the snapshot for 200, and the
validator replays and votes on slots 200-250 while catching up, but then
reboots again before catching up. When the validator reboots again, assume it
gets a snapshot for slot 300. It will not see its votes for slots 200-250 in
its vote account in bank 300, so it will have to assume that they were for
another fork, and wait for their lockouts to expire accordingly. Avoiding this
situation requires the caught up check and the correct cluster check (a wrong
cluster could also be missing the votes).


## Snapshot Verification Overview

A booting validator needs to have some set of trusted validators whose votes it
will rely upon to ensure that it has caught up to the cluster and has a valid
snapshot. With that set, the validator can follow the procedure detailed in the
`Boot Procedure` section.

For the sections below:

Assume an arbitrary `Tower` state `tower`, a `snapshot_root`, and a
`trusted_bank` (defined in step 8-1 of the `Boot Procedure` section).

Define `tower_vote_slot` to be: The slot of some vote in `tower`.

Define the function `is_ancestor(a, b)` to be: Given a slot `a` and a slot `b`
will return true if `a` is an ancestor of `b`.

Define the function `is_locked(a, b)` to be: `a + a.lockout <= b`

### Additions to a Snapshot

The snapshot is augmented to store the last `N` ancestors of `snapshot_root`.
These ancestors are incorporated into the bank hash so they can be verified
when a validator unpacks a snapshot. Call this set `ancestors`.

A set of votes that can prove lockout on `snapshot_root` is also added to the
snapshot package. Call this set `snapshot_votes`. The validator generating the
package will include a set of votes from each validator in the leader schedule.
For each included validator, the set of votes will be the votes on the fork
that achieves the highest lockout on `snapshot_root` (up to
`MAX_LOCKOUT_HISTORY`). The complete vote transaction will be included so that
the signature is verifiable.  If available, a vote for `snapshot_root` must be
included to verify `snapshot_root`'s bank hash.


## Boot Procedure:

1) The validator gets a snapshot to boot from. This can come from any source,
as it will be verified before being vote upon.

2) We reject any snapshots where `snapshot_root` is less than the root of
`tower` and `snapshot_root` is not in the list of roots in blocktree because
this means `snapshot_root` is for a different fork than the one this validator
last rooted. Thus it's critical for consistency in `replay_stage` that the
order of events when setting a new root is:

    1) Write the root to tower

    2) Write the root to blocktree

    3) Generate snapshot for that root

3) On startup, the validator checks that the current root in tower exists in
blocktree. If not (there was a crash between 2-1 and 2-2), then rewrite the
root to blocktree.

4) On startup, the validator checks that `snapshot_votes` includes votes that
prove threshold commitment from the trusted validators on `snapshot_root`. See
the `Trusted Validator Set` section for details. If not, the validator panics
and different snapshot must be obtained.

5) On startup, the validator boots from the `snapshot_root`, then replays all
descendant blocks of `snapshot_root` that exist in this validator's ledger,
building banks which are then stored in an output `bank_forks`. This is done in
`blocktree_processor.rs`. The root of this `bank_forks` is set to
`snapshot_root`.

6) On startup, the validator calculates the `first_votable_slot` from which it
is not locked out for voting. Every validator persists its tower state and must
consult this state in order to boot safely and resume from a snapshot without
being slashed. From this tower state and the ancestry information embedded in
the snapshot, a validator can derive which banks, are "safe" (See the
`Determining Vote Safety From a Snapshot and Tower` section for more details)
to vote for.

7) Periodically send canary transactions to the cluster using the validator's
local recent blockhash.

8) Wait for the following criteria. While waiting, set a new root every time
2/3 of the cluster's stake roots a bank. This allows the validator to prune its
state and also calculate leader schedules as it moves across epochs. Even if
the validator appears on the leader schedule, it does not produce blocks while
waiting.

   1) Observe a canary transaction in a bank that some threshold of trusted
   validator stake has voted on at least once. Call this `trusted_bank`.

   2) The current working bank has `bank.slot > first_votable_slot`

9) Start the normal voting process, voting for any slot that satisfies
`bank.slot > first_votable_slot`


## Trusted Validator Set

The booting validator needs to have a set of trusted validators that it uses to
validate the snapshot and that it is caught up.  To guarantee that the booting
validator connects to the cluster, the set needs to contain some threshold of
validators that are still participating in consensus. This excludes validators
that are running but are experiencing some attack (such as an eclipse attack)
that prevents them from receiving slots, voting, or broadcasting their votes.

### Verifying the Cluster is Building from the Snapshot

The booting validator can confirm that the snapshot is valid and that the
cluster it intends to join is building off of `snapshot_root` by verifying the
signatures of votes for `snapshot_root`. The validator can calculate the
commitment of its trusted validators, and at some threshold, such as 2/3 of
trusted validators are locked out at least 2^(MAX_LOCKOUT - 1) slots, accept
the snapshot.

**Proof:** For the snapshot to be valid, the validator that made the snapshot
must have rooted `snapshot_root`, meaning that it must have observed at least
2/3 of stake having >=2^(MAX_LOCKOUT - 1) lockout on `snapshot_root`. Any
validators that are participating in consensus will eventually also observe the
same lockout on `snapshot_root`, and will therefore bring themselves up to
>=2^(MAX_LOCKOUT - 1) lockout on `snapshot_root`. Thus with any set of trusted
validators meeting the conditions above, the booting validator will eventually
observe the threshold commitment and accept the snapshot.

### Verifying Booting Validator is Caught Up

The booting validator should send canary transactions as described in the `Boot
Procedure`. When the booting validator observes a canary transaction in a bank,
and it observes at least one vote for that bank from a threshold (ie 2/3) of
trusted validators, it can conclude that it has caught up to the trusted
validators.


## Determining `first_votable_slot` From a Snapshot and Tower

Define the "safety" condition to be:

Given a `BankForks`, `bank_forks`, a validator can run some procedure to
determine the `first_votable_slot` that it can vote on without violating the
lockouts of any vote in `tower`. The procedure for this is:

1) Find the earliest `vote` in the tower for which `is_ancestor(vote,
snapshot_root)` is not true.

2) `vote` and every vote after it needs to expire before the validator can vote
on any descendant of `snapshot_root`, so with all the votes `vote_i` in tower
where `vote_i >= vote`, `first_votable_slot = max(vote_i + lockout(vote_i))`.
If no `vote` exists, then `first_votable_slot = snapshot_root`.


## Achieving Safety

Define the "Safety Criteria" to be: If the validator has a `BankForks` and a
`Tower`, it is able to determine `is_ancestor(tower_vote_slot,
snapshot_descendant)` where `tower_vote_slot` is any slot in `Tower`, and
`snapshot_descendant` is any descendant of `snapshot_root` that is also present
in `bank_forks.banks`.

Assume the "Safety Criteria" is true, we show we can then achieve the "safety"
condition:

**Proof:** A validator wants to determine whether it can vote on
`trusted_bank`. `trusted_bank` must be a descendant of `snapshot_root` because
the validator does not play any state for non-descendants of `snapshot_root`
when booting from the snapshot. From assuming "Safety Criteria" above, this
means for any `tower_vote_slot` we can determine `is_ancestor(tower_vote_slot,
trusted_bank)`. This means we have all the tools to run the algorithm from the
safety definition.

Thus to achieve safety, we want to design the snapshotting system such that the
"Safety Criteria" is met.


## Implementing "Safety Criteria"

We implement the "Safety Criteria" in cases:

### Case 1: Calculating `is_ancestor(tower_vote_slot, snapshot_descendant)`
when `tower_vote_slot` < `snapshot_root`:

There are two variants depending on whether the list of `ancestors` goes back
far enough to include `tower_vote_slot`. This can be established by comparing
`tower_vote_slot` to the oldest slot in `ancestors`, `oldest_ancestor`.

#### Variant 1: `tower_vote_slot` >= `oldest_ancestor`:

##### Protocol:

Search the list of `ancestors` ancestors of `snapshot_root` to check if
`tower_vote_slot` is an ancestor of `snapshot_root`.

##### Proof of Correctness for Protocol:

This case is equivalent to determining `is_ancestor(tower_vote_slot,
snapshot_root)`. This is because `is_ancestor(tower_vote_slot,
snapshot_descendant) <=> (is_ancestor(tower_vote_slot, snapshot_root) &&
is_ancestor(snapshot_root, snapshot_descendant))`, and
`is_ancestor(snapshot_root, snapshot_descendant)` is true by the definition of
`snapshot_descendant`.

Now we show the protocol is sufficient to determine
`is_ancestor(tower_vote_slot, snapshot_root)`.  Because `tower_vote_slot + N >=
snapshot_root` in this variant, and `ancestors` has length `N`, then if
`tower_vote_slot`, is an ancestor, it has to be a member of `ancestors`, so the
protocol is sufficient.

#### Variant 2: `tower_vote_slot` < `oldest_ancestor`:

##### Protocol:

Search the vote state of the validator in `snapshot_descendant` for
`tower_vote_slot`. If `tower_vote_slot` is not found, assume it is not an
ancestor.

##### Proof of Correctness for Protocol:

A vote for a slot will only be placed into the validator's vote state in a bank
descended from the slot the vote is for. Therefore, a vote will only be found
in a vote state if the vote is for an ancestor of the slot the vote state is
found in. This protocol may falsely determine that `tower_vote_slot` is not an
ancestor of `snapshot_descendant` if the vote was never accepted by a leader,
but the result will be that the validator will take the more cautious approach
of waiting for the lockout of the vote to expire, so it will not be slashed due
to this failure.

### Case 2: Calculating `is_ancestor(tower_vote_slot, snapshot_descendant)`
when `tower_vote_slot` >= `snapshot_root`:

##### Protocol:

If `snapshot_descendant < tower_vote_slot`, return false, because an ancestor
cannot have a greater slot number. Otherwise, Let the bank state for slot
`snapshot_descendant` be called `snapshot_descendant_bank`. Check
`snapshot_descendant_bank.ancestors().contains(tower_vote_slot)`.

##### Proof of Correctness for Protocol:

**Lemma 1:** Given `bank_forks` and its root `bf_root`, the bank state
`snapshot_descendant_bank` for some slot `snapshot_descendant`, where
`snapshot_descendant_bank` is present in `bank_forks.banks`,
`snapshot_descendant_bank.ancestors()` must include all ancestors of
`snapshot_descendant_bank` that are `>= bf_root`

**Proof:** Let `bf_root` be the latest root bank in `bank_forks`. The lemma
holds at boot time because `bf_root == snapshot_root` and by step 4 of the
"Boot Procedure", `bank_forks` will only contain descendants of
`snapshot_root`, so each descendants' `ancestors` will only contain ancestors
`>= bf_root`. Going forward, by construction, ReplayStage only adds banks to
`bank_forks` if all of its ancestors are present and frozen. Thus because
`snapshot_descendant_bank` is present in `bank_forks.banks`,
`snapshot_descendant_bank.ancestors()` must include all frozen ancestors `>=
bf_root`. Furthermore, `bank_forks` prunes ancestors in its set of `banks` that
are `< bf_root_new` after setting a new root `bf_root_new`, so this invariant
is always true.

**Lemma 2:** Given a root bank `bf_root` of `bank_forks` and some
`tower_vote_slot` in tower, if `tower_vote_slot >= snapshot_root`, then
`tower_vote_slot >= bf_root`.

**Proof:** When we boot from a snapshot, we set `bf_root == snapshot_root`, and
in this case because we are assuming `tower_vote_slot > snapshot_root`, then we
know `tower_vote_slot >= bf_root` at bootup. Then when we set a new root in
`bank_forks` `bf_root_new` after booting, we guarantee this only happens if the
tower root is also set to `bf_root_new`. By the construction of tower, all
votes in the tower must be greater than the tower root, so `tower_vote_slot >=
bf_root_new`.

For `Case 2` we assumed `tower_vote_slot` >= `snapshot_root`, so from Lemma 2
we know `tower_vote_slot` >= `bf_root`.  Then from Lemma 1 we know its
sufficient to check
`snapshot_descendant_bank.ancestors().contains(tower_vote_slot)`.


