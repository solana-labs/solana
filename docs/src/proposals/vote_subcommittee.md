---
title: Vote Subcommittee 
---

## Problem

Each additional validator adds 1 vote per slot, increasing the
message and compute load on the network.

## Solution Overview

Under the assumption that Turbine is able to guarantee that all
honest nodes receive the block as long as at least X honest nodes
receive the block, it is possible to allow a sampled set of validators
to vote on that block as apposed to all the validators, and achieve
the same liveness properties. If the network is tolerant of 1/3
failures, then for a large enough random sample, like 200 or more,
the probability of containing 2/3+ faulty nodes is 1:10^40. Assuming
at least 1/3+ honest nodes are present in the subcommittee, then
faulty forks will not get finality, and network will be able to
continue.

## Detailed Solution The following sections provide more details
of the design.

## Definitions

* Voting Subcommittee: the set of nodes currently voting on blocks

* Voting epoch: the number of slots that the voting subcommittee
is voting for

* primary subcommittee: The half of the voting subcommittee that
is scheduled for its second epoch.

* secondary subcommittee: The half of the voting subcommittee that
is scheduled for its first epoch.

* rotation block: The block in the previous epoch that was rooted
by 2/3+ of both primary and secondary super-majorities.

* random sample: 200 nodes picked with a stake weighed shuffle.
Assuming 1/3 are faulty, probability of 133 or greater faulty nodes
is 1:10^20

* `slow_hash`: repeated sha256 for rounds equal to 1ms on modern
sha-ni hardware. 10^20 * 1ms / 1 billion cores ~= 10^9 years


### Safety Violations

An Optimistic Confirmation violation occurs if a subcommittee
optimistically confirms a slot that wasn't rooted by the following
subcommittee.

A Safety violation occurs if a subcommittee roots a slot that wasn't
rooted by the following subcommittee.

### Subcommittee Rotation

Voting Subcommittee is scheduled for a voting epoch, and is composed
of 2 subcommittees, A and B, scheduled in a staggered rotation.

``` 
A1 A1 A2 A2 A3
   B1 B1 B2 B2
```

The first epoch the subcommittee runs, it is considered **secondary**.
For it's second epoch, it is considered **primary**.

For a new secondary subcommittee to start, it must start with a
block that is a descendant of a **rotation block** that was confirmed
by both super-majorities in the previous epoch. The seed to derive
the new secondary subcommittee is `slow_hash(bank_hash super-majority
block)`.

### Threshold Switching

Secondary subcommittee is can use the primary's votes as a switching
threshold proof.

### Safety

```
A1 A1 A2 A2 A3
   B1 B1 B2 B2
```

To switch subcommittees both currently active primary and secondary
must confirm the same slot with a 2/3+ super-majority. When voting
subcommittee switches from (A1,B1) to (A2,B1), B1 must have rooted
the same fork as A1, and (A2,B1) must include the same fork, or a
lockout violation has occurred, or the network has stalled.

### Optimistic Confirmation

A slot is optimistically confirmed if and only if both subcommittees
confirm it with 2/3+ majority.

Voting subcommittee (A1,B1) confirms the last slot `X` of its
epoch, (A2,B1) take over on the same epoch. B1 must root `X`, or B1
has committed a threshold switch violation. Since B1 is primary,
it can only use its own votes for a switching proof.
