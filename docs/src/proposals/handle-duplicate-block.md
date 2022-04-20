---
title: Handle Duplicate Block
---

# Leader Duplicate Block Slashing

This design describes how the cluster slashes leaders that produce duplicate
blocks.

Leaders that produce multiple blocks for the same slot increase the number of
potential forks that the cluster has to resolve.

## Primitives
1. gossip_root: Nodes now gossip their current root
2. gossip_duplicate_slots: Nodes can gossip up to `N` duplicate slot proofs.
3. `DUPLICATE_THRESHOLD`: The minimum percentage of stake that needs to vote on a fork with version `X` of a duplicate slot, in order for that fork to become votable.

## Protocol
1. When WindowStage detects a duplicate slot proof `P`, it checks the new `gossip_root` to see if `<= 1/3` of the nodes have rooted a slot `S >= P`. If so, it pushes a proof to `gossip_duplicate_slots` to gossip. WindowStage then signals ReplayStage about this duplicate slot `S`. These proofs can be purged from gossip once the validator sees > 2/3 of people gossiping roots `R > S`.

2. When ReplayStage receives the signal for a duplicate slot `S` from `1)` above, the validator monitors gossip and replay waiting for`>= DUPLICATE_THRESHOLD` votes for the same hash which implies the same version of the slot. If this conditon is met for some version with hash `H` of slot `S`, this is then known as the `duplicate_confirmed` version of the slot.

Before a duplicate slot `S` is `duplicate_confirmed`, it's first excluded from the vote candidate set in the fork choice rules. In addition, ReplayStage also resets PoH to the *latest* ancestor of the *earliest* `non-duplicate/confirmed_duplicate_slot`, so that block generation can start happening on the earliest known *safe* block.

Some notes about the `DUPLICATE_THRESHOLD`. In the cases below, assume `DUPLICATE_THRESHOLD = 52`:

a) If less than `2 * DUPLICATE_THRESHOLD - 1` percentage of the network is malicious, then there can only be one such `duplicate_confirmed` version of the slot. With `DUPLICATE_THRESHOLD = 52`, this is
a malcious tolerance of `4%`

b) The liveness of the network is at most `1 - DUPLICATE_THRESHOLD - SWITCH_THRESHOLD`. This is because if you need at least `SWITCH_THRESHOLD` percentage of the stake voting on a different fork in order to switch off of a duplicate fork that has `< DUPLICATE_THRESHOLD` stake voting on it, and is *not* `duplicate_confirmed`. For `DUPLICATE_THRESHOLD = 52` and `DUPLICATE_THRESHOLD = 38`, this implies a liveness tolerance of `10%`.

For example in the situation below, validators that voted on `2` can't vote any further on fork `2` because it's been removed from fork choice. Now slot 6 better have enough stake for a switching proof, or the network halts.

```text
    |-------- 2 (51% voted, then detected this slot was a duplicate and removed this slot from fork choice)
0---|
    |---------- 6 (39%)

```

3. Switching proofs need to be extended to allow including vote hashes from different versions of the same same slot (detected through 1). Right now this is not supported since switching proofs can
only be built using votes from banks in BankForks, and two different versions of the same slot cannot
simultaneously exist in BankForks. For instance:

```text
    |-------- 2
    |
0------------- 1 ------ 2'
    |
    |---------- 6

```

Imagine each version of slot 2 and 2' have `DUPLICATE_THRESHOLD / 2` of the votes on them, so neither duplicate can be confirmed. At most slot 6 has `1 - DUPLICATE_THRESHOLD / 2` of the votes
on it, which is less than the switching threshold. Thus, in order for validators voting on `2` or `2'` to switch to slot 6, and make progress, they need to incorporate votes from the other version of the slot into their switching proofs.


### The repair problem.
Now what happens if one of the following occurs:

1) Due to network blips/latencies, some validators fail to observe the gossip votes before they are overwritten by newer votes? Then some validators may conclude a slot `S` is `duplicate_confirmed` while others don't.

2) Due to lockouts, no version of duplicate slot `S` reaches `duplicate_confirmed` status, but one of its descendants may reach `duplicate_confirmed` after those lockouts expire, which by definition, means `S` is also `duplicate_confirmed`.

3) People who are catching up and don't see the votes in gossip encounter a dup block and can't make progress.

We assume that given a network is eventually stable, if at least one correct validator observed `S` is `duplicate_confirmed`, then if `S` is part of the heaviest fork, then eventually all validators will observe some descendant of `S` is duplicate confirmed.

This problem we need to solve is modeled simply by the below scenario:

```text
1 -> 2 (duplicate) -> 3 -> 4 (duplicate)
```
Assume the following:

1. Due to gossiping duplciate proofs, we assume everyone will eventually see duplicate proofs for 2 and 4, so everyone agrees to remove them from fork choice until they are `duplicate_confirmed`.

2. Due to lockouts, `> DUPLICATE_THRESHOLD` of the stake votes on 4, but not 2. This means at least `DUPLICATE_THRESHOLD` of people have the "correct" version of both slots 2 and 4.

3. However, the remaining `1-DUPLICATE_THRESHOLD` of people have wrong version of 2. This means in replay, their slot 3 will be marked dead, *even though the faulty slot is 2*. The goal is to get these people on the right fork again.

Possible solution:

1. Change `EpochSlots` to signal when a bank is frozen, not when a slot is complete. If we see > `DUPLICATE_THRESHOLD` have frozen the dead slot 3, then we attempt recovery. Note this does not mean that all `DUPLICATE_THRESHOLD` have frozen the same version of the bank, it's just a signal to us that something may be wrong with our version of the bank.

2. Recovery takes the form of a special repair request, `RepairDuplicateConfirmed(dead_slot, Vec<(Slot, Hash)>)`, which specifies a  dead slot, and then a vector of `(slot, hash)` of `N` of its latest parents.

3. The repairer sees this request and responds with the correct hash only if any element of the `(slot, hash)` vector is both `duplicate_confirmed` and the hash doesn't match the requester's hash in the vector.

4. Once the requester sees the "correct" hash is different than their frozen hash, they dump the block so that they can accept a new block, and ask the network for the block with the correct hash.

Of course the repairer might lie to you, and you'll get the wrong version of the block, in which case you'll end up with another dead block and repeat the procedure.
