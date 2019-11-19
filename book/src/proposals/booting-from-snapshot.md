# Booting From a Snapshot

## Problem

A validator booting from a snapshot should confirm that they are caught up to the correct cluster before voting. There are two major risks if they do not:

### Wrong Cluster

The validator may be fed a snapshot and gossip info for a different (though not necessarily malicious) cluster. If that cluster is at slot 100,000 while the cluster the validator intended to join is at 10,000, if the validator votes once on the wrong cluster, it is locked out of the correct cluster for 90,000 slots.

### Voting Before Catching Up

Any vote that a validator signs must have its lockout observed. If a validator has to reboot, the most secure way for it to check if its lockouts apply to the new set of forks is to see the votes on the ledger. If it can do this, it knows that those votes do not lock it out of voting on the current fork, so it just needs to wait for the lockouts of the votes not on the ledger to expire. However, if the validator votes before catching up, the votes will not go onto the ledger, so if the validator reboots, it will have to assume that the votes lock it out from voting again.

## Snapshot Verification Overview

A booting validator needs to have some set of trusted validators whose votes it will rely upon to ensure that it has caught up to the cluster and has a valid snapshot. With that set, the validator can

1) Get a snapshot to boot from. This can come from any source, as it will be verified before being vote upon. Call the bank this snapshot is of, `S`.

2) Periodically send canary transactions with its local recent blockhash.

3) While waiting to see one of the canary transactions, set a new root every time some threshold percent of the trusted validator stake roots a bank. This allows the validator to prune its state and also calculate leader schedules as it moves across epochs.

4) Wait to observe a canary transaction in a bank that some threshold of trusted validator stake has voted on. Call this trusted bank `T`.

5) Figure out what banks the validator is not locked out from based on it's locktower. Every validator persists its locktower state and must consult this state in order to boot safely and resume from a snapshot without being slashed. From this locktower state and the ancestry information embedded in the snapshot, a validiator can derive which banks, are "safe"
(See the `Determining Vote Safety From a Snapshot and Locktower` section for more detals) to vote for.

6) Start voting for any descendant of a trusted bank `T` that is also "safe"


For the sections below:

Assume an arbitrary locktower state `L`, a snapshot root `S`, and a trusted bank slot `T` (defined in step 4 of the `Snapshot Verification Overview` section).

Define `L_i` to be: The slot of the ith vote in `L`.

Define the function `is_ancestor(a, b)` to be: Given a slot `a` and a slot `b` will return true if `a` is an ancestor of `b`.

Define the function `is_locked(a, b)` to be: `a + a.lockout <= b`

## Determining Vote Safety From a Snapshot and Locktower

Define the "safety" condition to be: 

Given a `BankForks`, a validator can run some procedure to determine whether it can vote on a frozen bank `T` (frozen banks must be present in `BankForks.banks`), without violating the lockouts of any vote in `L`. The procedure for this is:

1) If `T` < last locktower vote, return false,

2) Apply `T` to locktower state `L`. Pop off all votes `S_i` where `!is_locked(S_i, T)`. `T` must now be the top of the tower.

3) If for any remaining vote `L_i` in `L` `!is_ancestor(L_i, T)`, return false.

4) Otherwise, return true.

## Achieving Safety

Define the "Safety Criteria" to be: Given a `BankForks` any descendant of `S`, `S_d` that has a bank `B_d`  that is present in `BankForks.banks`, and any slot `L_i` in locktower, the validator is able to determine `is_ancestor(L_i, S_d)`.

Assume the "Safety Criteria" is true, we show we can then achieve the "safety" condition:

**Proof:** 
A validator wants to determine whether it can vote on `T`. `T` must be a descendant of `S` because the validator does not play any state for non-descendants of `S` when booting from the snapshot. From assuming "Safety Criteria" above, this means for any `L_i` we can determine `is_ancestor(L_i, T)`. This means we have all the tools to run the algorithm from the safety definition.

Thus to achieve safety, we want to design the snapshotting system such that the "Safety Criteria" is met.

## Implementing "Safety Criteria"

"Range Assumption": In our implementation, we assume that at the time the validator starts from the snapshot, `L_i + N >= S` for some large `N`. 

Snapshot Design:

The snapshot is augmented to store the last `N` ancestors of `S`. These ancestors are  incorporated into the bank hash so they can be verified when a validator unpacks a snapshot. Call this set `N_Ancestors`.

Boot Procedure: 

1) We reject any snapshots `S` where `S` is less than the root of `L` and `S` is not in the list of roots in blocktree because this means `S` is for a different fork than the one this validiator last rooted. Thus it's critical for consistency in `replay_stage` that the order of events when setting a new root is:

    1. Write the root to locktower
    2. Write the root to blocktree
    3. Generate snapshot for that root

2) On startup, make sure the current root in locktower exists in blocktree, if not (there was a crash between 1-1 and 1-2), then rewrite the root to blocktree.

3) On startup, the validator boots from the snapshot `S`, then replays all descendant blocks of `S` that exist in this validator's ledger, building banks which are then stored in an output `BankForks`. This is done in `blocktree_processor.rs`. The root of this `BankForks` is set to `S`.


We now implement the "Safety Criteria" in cases:

### Case 1: Calculating `is_ancestor(L_i, S_d)` when `L_i` < `S`:

#### Protocol:

Search the list of `N_Ancestors` ancestors of `S` to check if `L_i` is an ancestor of `S`.

#### Proof of Correctness for Protocol:
This case is equivalent to determining `is_ancestor(L_i, S)`. This is because `is_ancestor(L_i, S_d) <=> ( is_ancestor(L_i, S) && is_ancestor(S, S_d))`, and `is_ancestor(S, S_d)` is true by the definition of `S_d`.

Now we show the protocol  is sufficient to determine `is_ancestor(L_i, S)`. Because `L_i + N >= S` by the "Range Assumption", and `N_Ancestors` has length `N`, then if `L_i`,is an ancestor, it has to be a member of `N_Ancestors`, so the protocol is sufficient.


### Case 2: Calculating `is_ancestor(L_i, S_d)` when `L_i` >= `S`:

#### Protocol: 

If `S_d < L_i`, return false, because an ancestor cannot have a greater slot number. Otherwise,
Let the bank state for slot `S_d` be called `B_d`. Check `B_d.ancestors().contains(L_i)`.

#### Proof of Correctness for Protocol:

**Lemma 1:** Given any `BankForks` and its root `R`, the bank state `B_d` for some slot `S_d`, where `B_d` is present in `BankForks.banks`, `B_d.ancestors()` must include all ancestors of `B_d` that are `>= R`

**Proof:** Let `R` be the latest root bank in `BankForks`. The lemma holds at boot time because    `R == S` and by step 3 of the "Boot Procedure", BankForks will only contain descendants of `S`,so each descendants' `ancestors` will only contain ancestors `>= R`. 
Going forward, by construction, ReplayStage only adds banks to `BankForks` if all of its ancestors are present and frozen. Thus because `B_d` is present in `BankForks.banks`, `B_d.ancestors()` must include all frozen ancestors `>= R`. Furthermore, `BankForks` prunes ancestors in its set of `banks` that are `< R'` after setting a new root `R'`, so this invariant is always true.

**Lemma 2:** Given a root bank `R` of `BankForks` and some `L_i` in locktower, if `L_i >= S`, then `L_i >= R`.

**Proof:** When we boot from a snapshot, we set `R == S`, and in this case because we are assuming `L_i > S`, then we know `L_i >= R` at bootup. Then when we set a new root in BankForks `R'` after booting, we guarantee this only happens if the locktower root is also set to `R'`. By the construction of locktower, all votes in the tower must be greater than the locktower root, so `L_i >= R'`. 

For `Case 2` we assumed `L_i` >= `S`, so from Lemma 2 we know `L_i` >= `R`. Then from Lemma 1 we know its sufficient to check `B_d.ancestors().contains(L_i)`.







