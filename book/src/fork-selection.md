# Fork Selection This article describes Solana's *Fork Selection* algorithm
based on lockouts. It attempts to solve the following problems:

* Some forks may not end up accepted by the super-majority of the cluster, and
voters need to recover from voting on such forks.

* Many forks may be votable by different voters, and each voter may see a
different set of votable forks.  The selected forks should eventually converge
for the network.

* Reward based votes have an associated risk.  Voters should have the ability to
configure how much risk they take on.

* The [cost of rollback](#cost-of-rollback) is important to clients that rely on
a measure of Consistency.  It needs to be computable, and increase
super-linearly for older votes.

* ASIC speeds are different between nodes, and attackers could employ Proof of
History ASICS that are much faster than the rest of the network.  Consensus
needs to be resistant to attacks that exploit the variability in Proof of
History ASIC speed.

## Time

The Solana cluster generates a source of time via a Verifiable Delay Function we
are calling [Proof of History](book/src/synchronization.md).

Proof of History is used to create a deterministic round robin schedule for all
the active leaders.  At any given time only 1 leader, which can be computed from
the ledger itself, can propose a fork.  For more details, see [fork
generation](fork-generation.md) and [leader rotation](leader-rotation.md).

## Lockouts

The purpose of the lockout is to force a voter to commit opportunity cost to a
specific fork.  Lockouts are measured in slots, and therefor represent a
real-time forced delay that a voter needs to wait before breaking the commitment
to a fork.

Voters that violate the lockouts and vote for a diverging fork within the
lockout should be punished. The proposed punishment is to slash the voters stake
if a concurrent vote within a lockout for a non-descendant fork can be proven to
the cluster.

## Algorithm

The basic idea to this approach is to stack consensus votes and double lockouts.
Each vote in the stack is a confirmation of a fork.  Each confirmed fork is an
ancestor of the fork above it.  Each consensus vote has a `lockout` in units of
slots before the validator can submit a vote that does not contain the confirmed
fork as an ancestor.

When a vote is added to the stack, the lockouts of all the previous votes in the
stack are doubled (more on this in [Rollback](#Rollback)).  With each new vote,
a voter commits the previous votes to an ever-increasing lockout.  At 32 votes
we can consider the vote to be at `max lockout` any votes with a lockout equal
to or above `1<<32` are dequeued (FIFO).  Dequeuing a vote is the trigger for a
reward.  If a vote expires before it is dequeued, it and all the votes above it
are popped (LIFO) from the vote stack.  The voter needs to start rebuilding the
stack from that point.

### Rollback

Before a vote is pushed to the stack, all the votes leading up to vote with a
lower lock time than the new vote are popped.  After rollback lockouts are not
doubled until the voter catches up to the rollback height of votes.

For example, a vote stack with the following state:

| vote | vote time | lockout | lock expiration time |
|-----:|----------:|--------:|---------------------:|
|    4 |         4 |      2  |                    6 |
|    3 |         3 |      4  |                    7 |
|    2 |         2 |      8  |                   10 |
|    1 |         1 |      16 |                   17 |

*Vote 5* is at time 9, and the resulting state is

| vote | vote time | lockout | lock expiration time |
|-----:|----------:|--------:|---------------------:|
|    5 |         9 |      2  |                   11 |
|    2 |         2 |      8  |                   10 |
|    1 |         1 |      16 |                   17 |

*Vote 6* is at time 10

| vote | vote time | lockout | lock expiration time |
|-----:|----------:|--------:|---------------------:|
|    6 |        10 |       2 |                   12 |
|    5 |         9 |       4 |                   13 |
|    2 |         2 |       8 |                   10 |
|    1 |         1 |      16 |                   17 |

At time 10 the new votes caught up to the previous votes.  But *vote 2* expires
at 10, so the when *vote 7* at time 11 is applied the votes including and above
*vote 2* will be popped.

| vote | vote time | lockout | lock expiration time |
|-----:|----------:|--------:|---------------------:|
|    7 |        11 |       2 |                   13 |
|    1 |         1 |      16 |                   17 |

The lockout for vote 1 will not increase from 16 until the stack contains 5
votes.

### Slashing and Rewards

Voters should be rewarded for selecting the fork that the rest of the network
selected as often as possible.  This is well-aligned with generating a reward
when the vote stack is full and the oldest vote needs to be dequeued.  Thus a
reward should be generated for each successful dequeue.

### Cost of Rollback

Cost of rollback of *fork A* is defined as the cost in terms of lockout time to
the validators to confirm any other fork that does not include *fork A* as an
ancestor.

The **Economic Finality** of *fork A* can be calculated as the loss of all the
rewards from rollback of *fork A* and its descendants, plus the opportunity
cost of reward due to the exponentially growing lockout of the votes that have
confirmed *fork A*.

### Thresholds

Each voter can independently set a threshold of network commitment to a fork
before that voter commits to a fork.  For example, at vote stack index 7, the
lockout is 256 time units.  A voter may withhold votes and let votes 0-7 expire
unless the vote at index 7 has at greater than 50% commitment in the network.
This allows each voter to independently control how much risk to commit to a
fork.  Committing to forks at a higher frequency would allow the voter to earn
more rewards.

### Algorithm parameters

These parameters need to be tuned.

* Number of votes in the stack before dequeue occurs (32).
* Rate of growth for lockouts in the stack (2x).
* Starting default lockout (2).
* Threshold depth for minimum network commitment before committing to the fork
  (8).
* Minimum network commitment size at threshold depth (50%+).

### Free Choice

A "Free Choice" is an unenforcible voter action.  A voter that maximizes
self-reward over all possible futures should behave in such a way that the
system is stable, and the local greedy choice should result in a greedy choice
over all possible futures.  A set of voter that are engaging in choices to
disrupt the protocol should be bound by their stake weight to the denial of
service.  Two options exits for voter:

* a voter can outrun previous voters in virtual generation and submit a
  concurrent fork

* a voter can withhold a vote to observe multiple forks before voting

In both cases, the voters in the network have several forks to pick from
concurrently, even though each fork represents a different height.  In both
cases it is impossible for the protocol to detect if the voter behavior is
intentional or not.

### Greedy Choice for Concurrent Forks

When evaluating multiple forks, each voter should use the following rules:

1. Forks must satisify the *Threshold* rule.

2. Pick the fork that maximizes the total cluster lockout time for all the
ancestor forks.

3. Pick the fork that has the greatest amount of cluster transaction fees.

4. Pick the latest fork in terms of PoH.

Cluster transaction fees are fees that are deposited to the mining pool as
described in the [Staking Rewards](book/src/staking-rewards.md) section.

## PoH ASIC Resistance

Votes and lockouts grow exponentially while ASIC speed up is linear.  There are
two possible attack vectors involving a faster ASIC.

### ASIC censorship

An attacker generates a concurrent fork that outruns previous leaders in an
effort to censor them. A fork proposed by this attacker will be available
concurrently with the next available leader.  For nodes to pick this fork it
must satisfy the *Greedy Choice* rule.

1. Fork must have equal number of validator votes for the ancestor fork.  
2. Fork cannot be so far a head as to cause expired votes.
3. Fork must have a greater amount of cluster transaction fees.

This attack is then limited to censoring the previous leaders fees, and
individual transactions.  But it cannot halt the network, or reduce the
validator set compared to the concurrent fork.  Fee censorship is limited to
access fees going to the leaders but not the validators.

### ASIC Rollback

An attacker generates a concurrent fork from an older block to try to rollback
the network.  In this attack the concurrent fork is competing with forks that
have already been voted on.  This attack is limited by the exponential growth of
the lockouts.

* 1 vote has a lockout of 2 slots.  Concurrent fork must be at least 2 slots
ahead, and be produced in 1 slot. Therefore requires an ASIC 2x faster.

* 2 votes have a lockout of 4 slots.  Concurrent fork must be at least 4 slots
ahead and produced in 2 slots. Therefore requires an ASIC 2x faster.

* 3 votes have a lockout of 8 slots.  Concurrent fork must be at least 8 slots
ahead and produced in 3 slots. Therefore requires an ASIC 2.6x faster.

* 10 votes have a lockout of 1024 slots.  1024/10, or 102.4x faster ASIC.

* 20 votes have a lockout of 2^20 slots.  2^20/20, or 52,428.8x faster ASIC.
