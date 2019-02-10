# Leader Rotation

At any given moment, a cluster expects only one fullnode to produce ledger
entries. By having only one leader at a time, all validators are able to replay
identical copies of the ledger. The drawback of only one leader at a time,
however, is that a malicious leader is capable of censoring votes and
transactions. Since censoring cannot be distinguished from the network dropping
packets, the cluster cannot simply elect a single node to hold the leader role
indefinitely. Instead, the cluster minimizes the influence of a malicious
leader by rotating which node takes the lead.

Each validator selects the expected leader using the same algorithm, described
below. When the validator receives a new signed ledger entry, it can be certain
that entry was produced by the expected leader.

## Leader Schedule Generation at Epoch

Leader schedule is generated at a predefined time, using a root fork.

1. The root checkpoint is updated as a validator votes for new forks.

2. When the root checkpoint slot height crosses the epoch boundary, the leader
schedule is updated for the next epoch.

For example:

The epoch is 100 slots. The root fork is updated from fork computed at slot 99
to a fork computed at slot 102. Slot 100,101 forks were skipped because of
failures.  The new leader schedule is computed using fork 102.  It is active
from slot 200 until it is updated.

If the next slot skips an epoch, it is due to a considerable network failure,
and the leader schedule from the previous epoch is still valid until the root
checkpoint is updated.

## Leader Schedule Generation at Genesis

The genesis block contains a single leader.  This leader is scheduled for the
first two epochs.  The length of the first two epochs can be specified in the
genesis block as well.  The minimum length of the first epochs must be equal or
greater then the maximum rollback depth as defined in [fork
selection](fork-selection.md).

## Leader Schedule Generation Algorithm

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

The lifetime of a leader schedule is called an *epoch*. The epoch is split into
*slots*, where each slot has a duration of `T` PoH ticks.

A leader transmits entries during its slot.  After `T` ticks, all the
validators switch to the next scheduled leader. Validators must ignore entries
sent outside a leader's assigned slot.

All `T` ticks must be observed by the next leader for it to build its own
entries on. If entries are not observed (leader is down) or entries are invalid
(leader is buggy or malicious), the next leader must produce ticks to fill the
previous leader's slot. Note that the next leader should do repair requests in
parallel, and postpone sending ticks until it is confident other validators
also failed to observe the previous leader's entries. If a leader incorrectly
builds on its own ticks, the leader following it must replace all its ticks.
