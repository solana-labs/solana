# Leader Rotation

The goal of this RFC is to define how leaders rotate in taking their role.

## Leader Schedule Generation

Leader schedule is decided via a predefined seed.  The process is as follows:

1. Periodically at a specific `PoH tick count` use the tick count (simple monotonically increasing counter) as a seed to a stable pseudo-random algorithm.
2. At that height, example the bank for all the currently staked accounts and their assigned leader identities and weights.
3. Sort them by stake weight.
4. Using the random seed select nodes weighted by stake to create a stake weighted ordering.
5. This ordering becomes valid in `N` number of slots.

The seed that is selected is predictable but unbiasable.  There is no grinding attack to influence its outcome.  To reduce the likelihood of censorship of leaders in the network the set of **staked accounts** is computed over a large period.

## Leader Rotation

* The leader is rotated every `T` PoH ticks (leader period), according to the leader schedule.  This amount of time as represented by the PoH ticks is called a slot.

Leader's transmit for a count of `T` PoH ticks.  When `T` is reached all the validators should switch to the next scheduled leader.  Leaders that transmit out of order can be ignored.

All `T` ticks must be observed from the current leader for that part of PoH to be accepted by the network.  If `T` ticks (and any intervening transactions) are not observed, the network optimistically fills in the `T` ticks, and continues with PoH from the next leader.  See [fork generation](rfcs/0002-fork-generation.md).

## Network Variables

`N` - Number of slots before a new leader schedule becomes active after it is computed.

`T` - Number of PoH ticks per leader slot.
