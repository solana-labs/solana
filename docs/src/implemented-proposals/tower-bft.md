---
title: Tower BFT
---

This design describes Solana's _Tower BFT_ algorithm. It addresses the following problems:

- Some forks may not end up accepted by the supermajority of the cluster, and voters need to recover from voting on such forks.
- Many forks may be votable by different voters, and each voter may see a different set of votable forks. The selected forks should eventually converge for the cluster.
- Reward based votes have an associated risk. Voters should have the ability to configure how much risk they take on.
- The [cost of rollback](tower-bft.md#cost-of-rollback) needs to be computable. It is important to clients that rely on some measurable form of Consistency. The costs to break consistency need to be computable, and increase super-linearly for older votes.
- ASIC speeds are different between nodes, and attackers could employ Proof of History ASICS that are much faster than the rest of the cluster. Consensus needs to be resistant to attacks that exploit the variability in Proof of History ASIC speed.

For brevity this design assumes that a single voter with a stake is deployed as an individual validator in the cluster.

## Time

The Solana cluster generates a source of time via a Verifiable Delay Function we are calling [Proof of History](../cluster/synchronization.md).

The unit of time is called a "slot". Each slot has a designated leader that can
produce a block `B`. The `slot` of block `B` is designated `slot(B)`. A leader
does not necessarily need to generate a block for its slot, in which case there
may not be blocks for some slots.

For more details, see [fork generation](../cluster/fork-generation.md) and [leader rotation](../cluster/leader-rotation.md).

## Votes

Validators communicate which fork they think is the heaviest through votes.
Each vote `v` is signed by the validator that produces it, and is of the form `(i, B)`, where `i` is the public key of the validator producing the vote and `B` is a hash identifying the block being voted for.

## Lockouts

Making votes on a particular fork incurs a lockout on that particular fork. A lockout is a designated period of time, measured in slots, within which a validator cannot vote on another fork. The purpose of the lockout is to force a
validator to commit opportunity cost to a specific fork. Lockouts are measured
in slots, and therefore represent a real-time forced delay that a validator
needs to wait before breaking the commitment to a fork.

Validators that violate the lockouts and vote for a diverging fork within the lockout should be punished. The proposed punishment is to slash the validator stake if a concurrent vote within a lockout for a non-descendant fork can be proven to the cluster.

## Algorithm

The basic idea to this approach is to stack consensus votes and double lockouts. Each vote in the stack is a confirmation of a fork. Each confirmed fork is an ancestor of the fork above it. Each vote has a `lockout` in units of slots before the validator can submit a vote that does not contain the confirmed fork as an ancestor.

We call this stack the Vote Tower.

When a vote is added to the tower, the lockouts of all the previous votes in the tower are doubled (more on this in [Vote Tower](#vote-tower)). With each new vote, a validator commits the previous votes to an ever-increasing lockout. At 32 votes we can consider the vote to be at `max lockout` any votes with a lockout equal to or above `1<<32` are dequeued \(FIFO\). Dequeuing a vote is the trigger for a reward. If the vote on the top of the tower expires before it is dequeued, it and subsequent expired votes are popped in a LIFO fashion from the vote tower. The validator needs to start rebuilding the tower from that point.

### Vote Tower

Before a vote is pushed to the tower, all the votes leading up to vote with a lower lock expiration slot than the new vote are popped. After rollback
lockouts are not doubled until the validator catches up to the rollback height of votes.

For example, a vote tower with the following state:

| vote | vote slot | lockout | lock expiration slot |
| ---: | --------: | ------: | -------------------: |
|    4 |         4 |       2 |                    6 |
|    3 |         3 |       4 |                    7 |
|    2 |         2 |       8 |                   10 |
|    1 |         1 |      16 |                   17 |

_Vote 5_ is at slot 9, and the resulting state is

| vote | vote slot | lockout | lock expiration slot |
| ---: | --------: | ------: | -------------------: |
|    5 |         9 |       2 |                   11 |
|    2 |         2 |       8 |                   10 |
|    1 |         1 |      16 |                   17 |

_Vote 6_ is at slot 10

| vote | vote slot | lockout | lock expiration slot |
| ---: | --------: | ------: | -------------------: |
|    6 |        10 |       2 |                   12 |
|    5 |         9 |       4 |                   13 |
|    2 |         2 |       8 |                   10 |
|    1 |         1 |      16 |                   17 |

At slot 10 the new votes caught up to the previous votes. When _vote 7_ at slot 11 is applied we scan top down to pop expired votes. Although _vote 2_ has expired, since _vote 6_ has not expired, we do not continue scanning. Finally we have reached a new stack depth, lockouts are doubled

| vote | vote slot | lockout | lock expiration slot |
| ---: | --------: | ------: | -------------------: |
|    7 |        11 |       2 |                   13 |
|    6 |        10 |       4 |                   14 |
|    5 |         9 |       8 |                   17 |
|    2 |         2 |      16 |                   18 |
|    1 |         1 |      32 |                   33 |

Finally we have _vote 8_ at slot 18, this leads to the expiry of _vote 7_, _vote 6_, and _vote 5_.

| vote | vote slot | lockout | lock expiration slot |
| ---: | --------: | ------: | -------------------: |
|    8 |        18 |       2 |                   20 |
|    2 |         2 |      16 |                   18 |
|    1 |         1 |      32 |                   33 |

### Cost of Rollback

Cost of rollback of _fork A_ is defined as the cost in terms of lockout time to the validator to confirm any other fork that does not include _fork A_ as an ancestor.

The **Economic Finality** of _fork A_ can be calculated as the loss of all the rewards from rollback of _fork A_ and its descendants, plus the opportunity cost of reward due to the exponentially growing lockout of the votes that have confirmed _fork A_.

### Threshold Check
In order to prevent a validator from locking itself out on the wrong fork
in the case of a partition, there also needs to be a check to ensure that the rest of the cluster is committing to the same fork. This check is called the
"threshold check", and is outlined as follows.

In deciding whether to vote for a block `B`:

1. Simulate a vote for `B` on your current tower
2. Simulate popping off all the votes that would be expired by `B`
3. Now index every vote in the tower from `[0, tower.length()]`, assuming that the most recent simulated vote `B` is index 0, the second most recent vote is index 1, etc.
4. Let `T` be the vote in the tower with index equal to `threshold_check_depth`, currently hardcoded to `8`.
5. Check all the blocks descended from `T`. Let `Votes` be the set of all votes in these blocks for `T` or any descendants `D_n` of `T`. Let `V` be the set of all validators that have made a vote in `V`. If the sum of the validators' stakes in `V` totals `>= 2/3` of the stake of the network, then we commit a vote to `T`.

### Algorithm parameters

The following parameters need to be tuned:

- Number of votes in the stack before dequeue occurs \(32\).
- Rate of growth for lockouts in the stack \(2x\).
- Starting default lockout \(2\).
- Threshold check depth for minimum cluster commitment before committing to the fork \(8\).
- Minimum cluster commitment size at threshold depth \(50%+\).

### Fork Choice

Fork choice is how each validator determines which fork to vote on when multiple
concurrent forks exist. Forks are weighted based on the latest votes made by the validator set, and individual validators then vote on the "heaviest"
such fork.

Given the view of a single validator `i`:

Let `V` be the set of "most recent" valid votes received by `i`, i.e., `v = (j, B)` is in `V` and `i` has not also received a vote of the form `(j, B′) `such that `slot(B′) > slot(B)`.

Now the algorithm proceeds as follows:

1. For each vote `(j, B)` in `V`, add the stake of `j` to `B` and all of its
ancestors.
2. Now Set `B` to be the rooted block. Set `finish := 0`.
3. Perform the following loop:

```
*While* `finish == 0`
*Do*:
    *If*: `i` has received no children of `B` then set `finish := 1` and return
    `B`.
    *Else*: Let `B′` be the child of `B` (amongst those received by `i`) with
    most the most stake-weighted votes in `V`, breaking ties by the smallest
    slot. Set `B` equal to `B'`.
```

### Voting Algorithm

Each validator maintains a vote tower `T` which follows the rules described above in [Vote Tower](#vote-tower), which is a sequence of blocks it has voted for (initially empty). The variable `l` records the length of the stack. For each entry in the tower, denoted by `B = T(x)` for `x < l` where `B` is the `xth` entry in the tower, we record also a value `confcount(B)`. Define the lock expiration slot `lockexp(B) := slot(B) + 2 ^ confcount(B)`.

The validator `i` runs a voting loop as as follows. Let `B` be the heaviest
block returned by the fork choice rule above [Fork Choice](#fork-choice). If `i` has not voted for `B` before, then `i` votes for `B` so long as the following conditions are satisfied:

1. Respecting lockouts: For any block `B′` in the tower that is not an ancestor of `B`, `lockexp(B′) ≤ slot(B)`.
2. Threshold check: Described above in [Threshold Check](#threshold-check)
3. Switching threshold: Have sufficiently many votes on other forks if switching forks. Let `Btop` denote the block at the top of the stack. If `Btop` is not an ancestor of `B`, then:
    - Let `VBtop ⊆ V` be the set of votes on `Btop` or ancestors or descendents of `Btop`.
    - We need `|V \ VBtop | > 38%`. More details on this can be found in [Optimistic Confirmation](../proposals/optimistic_confirmation.md)

If all the conditions are satisfied and validator `i` votes for block `B` then it adjusts its tower as follows (same rules described above in [Vote Tower](#vote-tower)).
1. Remove expired blocks top down. Let `x := l - 1`. While `x >= 0 && lockexp(T(x)) < slot(B)`, remove `T(x)` from the tower, and set `l := l - 1` and `x := x - 1`.
2. Add block to tower. `T(l) := B`, `confcount(B) := 1`, and set `l := l + 1`.
3. Double lockouts. For each element `B = T(x)` if `l > x + confcount(B)`, then `confcount(B) := confcount(B) + 1`.

## PoH ASIC Resistance

Votes and lockouts grow exponentially while ASIC speed up is linear. There are possible attack vectors involving a faster ASIC outlined below.

### ASIC Rollback

An attacker generates a concurrent fork from an older block to try to rollback the cluster. In this attack the concurrent fork is competing with forks that have already been voted on. This attack is limited by the exponential growth of the lockouts.

- 1 vote has a lockout of 2 slots. Concurrent fork must be at least 2 slots ahead, and be produced in 1 slot. Therefore requires an ASIC 2x faster.
- 2 votes have a lockout of 4 slots. Concurrent fork must be at least 4 slots ahead and produced in 2 slots. Therefore requires an ASIC 2x faster.
- 3 votes have a lockout of 8 slots. Concurrent fork must be at least 8 slots ahead and produced in 3 slots. Therefore requires an ASIC 2.6x faster.
- 10 votes have a lockout of 1024 slots. 1024/10, or 102.4x faster ASIC.
- 20 votes have a lockout of 2^20 slots. 2^20/20, or 52,428.8x faster ASIC.
