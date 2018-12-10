# Leader Rotation

The goal of this RFC is to define how fullnodes rotate in taking the leader role.

## Leader Schedule Generation

Leader schedule is decided via a predefined seed.  The process is as follows:

1. Periodically use the PoH tick height (a monotonically increasing counter) to seed a stable pseudo-random algorithm.
2. At that height, sample the bank for all the currently staked accounts, their assigned leader identities and weights.
3. Sort them by stake weight.
4. Use the random seed select nodes weighted by stake to create a stake weighted ordering.
5. This ordering becomes valid after a cluster-configured number of ticks.

The seed that is selected is predictable but unbiasable.  There is no grinding attack to influence its outcome.  To reduce the likelihood of censorship of leaders in the network the set of **staked accounts** is computed over a large period.

## Appending Entries

A leader schedule is split into *slots*, where each slot has a duration of `T` PoH ticks.

A leader transmits entries during its slot.  After `T` ticks, all the validators switch to the next scheduled leader. Validators must ignore entries sent outside a leader's assigned slot.

All `T` ticks must be observed from the current leader for that part of PoH to be accepted by the cluster.  If entries are not observed (leader is down) or entries are invalid (leader is buggy or malicious), a validator should fill the gap with empty entries and continue with PoH from the next leader.
