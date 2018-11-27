# Leader Rotation

The goal of this RFC is to define how leader nodes are rotated in Solana.

## Leader Schedule Generation

Leader schedule is decided via a predefined seed.  The process is as follows:

1. Periodically at a specific `PoH tick count` use the tick count (simple monotonically increasing counter) as a seed to a stable psudo-random algorithm.
2. At that height, compute all the currently staked accounts and their assigned leader identities and weights.
3. Sort them by stake weight.
4. Using the random seed select nodes weighted by stake to create a stake weighted ordering.
5. This ordering becomes valid in `N` `PoH tick counts`.

The seed that is selected is predictable but unbiasable.  There is no grinding attack to influence its outcome.  The set of **staked accounts** and their leader identities is computed over a large period, which is our approach to censorship resistance of the staking set.  If at least 1 leader in the schedule is not censoring staking transactions then over a long period of time that leader can ensure that the set of active nodes is not censored.

## Leader Rotation

* The leader is rotated every `T` PoH ticks (leader period), accoding to the leader schedule

Leader's transmit for a count of `T` PoH ticks.  When `T` is reached all the validators should switch to the next scheduled leader.  Leaders that transmit out of order can be ignored or slashed.

All `T` ticks must be observed from the current leader for that part of PoH to be accepted by the network.  If `T` ticks (and any intervening transactions) are not observed, the network optimistically fills in the `T` ticks, and continues with PoH from the next leader.  See [branch generation](rfcs/0002-branch_generation.md).

## Network Variables

`N` - Number of voting rounds for which a leader schedule is considered before a new leader schedule is used This number should be large and potentially cover 2 weeks.

`T` - number of PoH ticks per leader period.
