# Leader Rotation

At any given moment, a cluster expects only one fullnode to produce ledger
entries. By having only one leader at a time, all validators are able to replay
identical copies of the ledger. The drawback of only one leader at a time,
however, is that a malicious leader is cabable of censoring votes and
transactions. Since censoring cannot be distinguished from the network dropping
packets, the cluster cannot simply elect a single node to hold the leader role
indefinitely. Instead, the cluster minimizes the influence of a malcioius
leader by rotating which node takes the lead.

Each validator selects the expected leader using the same algorithm, described
below. When the validator receives a new signed ledger entry, it can be certain
that entry was produced by the expected leader.

## Leader Schedule Generation

Leader schedule is generated using a predefined seed.  The process is as follows:

1. Periodically use the PoH tick height (a monotonically increasing counter) to
   seed a stable pseudo-random algorithm.
2. At that height, sample the bank for all the staked accounts with leader
   identities that have voted within a cluster-configured number of ticks. The
   sample is called the *active set*.
3. Sort the active set by stake weight.
4. Use the random seed to select nodes weighted by stake to create a
   stake-weighted ordering.
5. This ordering becomes valid after a cluster-configured number of ticks.

The seed that is selected is predictable but unbiasable.  There is no grinding
attack to influence its outcome. The active set, however, can be biased by a
leader by censoring validator votes. To reduce the likelihood of censorship,
the active set is sampled many slots in advance, such that votes will have been
collected by multiple leaders. If even one node is honest, the malicious
leaders will not be able to use censorship to influence the leader schedule.

## Appending Entries

A leader schedule is split into *slots*, where each slot has a duration of `T`
PoH ticks.

A leader transmits entries during its slot.  After `T` ticks, all the
validators switch to the next scheduled leader. Validators must ignore entries
sent outside a leader's assigned slot.

All `T` ticks must be observed from the current leader for that part of PoH to
be accepted by the cluster. If entries are not observed (leader is down) or
entries are invalid (leader is buggy or malicious), a validator should fill the
gap with empty entries and continue with PoH from the next leader.
