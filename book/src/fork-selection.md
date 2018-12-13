# Fork Selection
 This RFC defines Solana's *Nakomoto Fork Selection* algorithm based on time locks. It satisfies the following properties:


* A voter can eventually recover from voting on a fork that doesn't become the fork with the desired finality.
* If the voters share a common ancestor then they will converge to a fork containing that ancestor no matter how they are partitioned, although it may not be the latest possible ancestor at the start of the fork.
* Rollback requires exponentially more time for older votes than for newer votes.
* Voters have the freedom to set a minimum finality threshold before committing a vote to a higher lockout.  This allows each voter to make a trade-off between risk and reward. See [finality](#economic-finality).

## Time

For networks like Solana, time can be the PoH hash count, which is a VDF that provides a source of time before consensus. Other networks adopting this approach would need to consider a global source of time.

For Solana, time uniquely identifies a specific leader for fork generation.  At any given time only 1 leader, which can be computed from the ledger itself, can propose a fork.  For more details, see [fork generation](fork-generation.md) and [leader rotation](leader-rotation.md).

## Algorithm

The basic idea to this approach is to stack consensus votes.  Votes in the stack must be for forks that descend from each other and for forks that are valid in the ledger they are submitted to.  Each consensus vote has a `lockout` in units of time before it can be discarded.  When a vote is added to the stack, the lockouts of all the previous votes in the stack are doubled (see [Rollback](#Rollback)).  With each new vote, a voter commits the previous votes to an ever-increasing lockout.  Since at 32 votes we can consider the system to be at `max lockout` any votes with a lockout above `1<<32` are dequeued (FIFO).  Dequeuing a vote is the trigger for a reward.  If a vote expires before it is dequeued, it and all the votes above it are popped (LIFO) from the vote stack.  The voter needs to start rebuilding the stack from that point.


### Rollback

Before a vote is pushed to the stack, all the votes leading up to vote with a lower lock time than the new vote are popped.  After rollback lockouts are not doubled until the voter catches up to the rollback height of votes.

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

At time 10 the new votes caught up to the previous votes.  But *vote 2* expires at 10, so the when *vote 7* at time 11 is applied the votes including and above *vote 2* will be popped.

| vote | vote time | lockout | lock expiration time |
|-----:|----------:|--------:|---------------------:|
|    7 |        11 |       2 |                   13 |
|    1 |         1 |      16 |                   17 |

The lockout for vote 1 will not increase from 16 until the stack contains 5 votes.

### Slashing and Rewards

The purpose of the lockout is to force a voter to commit opportunity cost to a specific fork.  Voters that violate the lockouts and vote for a diverging fork within the lockout should be punished.  Slashing or simply freezing the voter from rewards for a long period of time can be used as punishment.

Voters should be rewarded for selecting the fork that the rest of the network selected as often as possible.  This is well-aligned with generating a reward when the vote stack is full and the oldest vote needs to be dequeued.  Thus a reward should be generated for each successful dequeue.

### Economic Finality

Economic finality can be calculated as the total opportunity cost due to the vote lockout at a given fork.  As votes get further and further buried, the economic finality increases because the cost of unrolling would be the total loss of all the reward from the lockouts at that fork.

### Thresholds

Each voter can independently set a threshold of network commitment to a fork before that voter commits to a fork.  For example, at vote stack index 7, the lockout is 256 time units.  A voter may withhold votes and let votes 0-7 expire unless the vote at index 7 has at greater than 50% commitment in the network.  This allows each voter to independently control how much risk to commit to a fork.  Committing to forks at a higher frequency would allow the voter to earn more rewards.

### Algorithm parameters

These parameters need to be tuned.

* Number of votes in the stack before dequeue occurs (32).
* Rate of growth for lockouts in the stack (2x).
* Starting default lockout (2).
* Threshold depth for minimum network commitment before committing to the fork (8).
* Minimum network commitment size at threshold depth (50%+).

### Free Choice

A "Free Choice" is an unenforcible node action.  A node that maximizes self-reward over all possible futures should behave in such a way that the system is stable, and the local greedy choice should result in a greedy choice over all possible futures.  A set of nodes that are engaging in choices to disrupt the protocol should be bound by their stake weight to the denial of service.  Two options exits for nodes:

* a node can outrun previous nodes in virtual generation and submit a concurrent fork
* a node can withold a vote to observe multiple forks before voting

In both cases, the nodes in the network have several forks to pick from concurrently, even though each fork represents a different height.

### Greedy Choice for Concurrent Forks

When evaluating multiple forks, each node should pick the fork that will maximize economic finality, or the latest fork if all are equal.  Economic finality can be measured in terms of lockout on a fork.
