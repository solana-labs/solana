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
to vote on that block as opposed to all the validators, and achieve
the same liveness properties. If the network is tolerant of 1/3
failures, then for a large enough random sample, like 200 or more,
the probability of containing 2/3+ faulty nodes is 1:10^40. Assuming
at least 1/3+ honest nodes are present in the subcommittee, then
faulty forks will not get finality, and network will be able to
continue.

## Detailed Solution

The following sections provide more details of the design.

### Definitions

* Voting Subcommittee: the set of nodes currently voting on blocks

* Voting epoch: the number of slots that the voting subcommittee
is voting for. This is separate from the leader schedule epoch.

* primary subcommittee: The half of the voting subcommittee that
is scheduled for its second epoch.

* secondary subcommittee: The half of the voting subcommittee that
is scheduled for its first epoch.

* rotation seed: The seed used to generate the random sample of
nodes. `slow_hash(penultimate snapshot hash, voting epoch start slot)`

* `slow_hash`: repeated sha256 for rounds equal to 1ms on modern
sha-ni hardware.

* switch rotation: when a secondary subcommittee is switched

* flip rotation: when primary and secondary subcommittees switch

### Safety Violations

An Optimistic Confirmation violation occurs if a voting subcommittee
optimistically confirms a slot that wasn't rooted by the following
voting subcommittee.

A Safety violation occurs if a subcommittee roots a slot that wasn't
rooted by the following subcommittee.

All full nodes including RPCs should halt as soon as a safety
violation is detected.

### Subcommittee Rotation

```

a1 A1 A1 a1 a2 A2 A2 a2 a3
B1 b1 b2 B2 B2 b2 b3 B3 B3
```

To rotate, the subcommittee must go through the rotation protocol.
In the above example, lowercase letter represents **secondary**
while uppercase represents **primary**.

Each transition like (A1,b1) -> (A1, b2) occurs on blocks that cross
the epoch boundary and are descendants of a rooted block from the
previous epoch.

The transition starts at a bank that crosses the epoch boundary.
If it is a descendant of a rooted bank in the previous epoch then
the transition is active.  If the block is not a descendant, then
the transition is inactive.

There are two types of transitions

Flip: (a1,B1) -> (A1, b1)
* active blocks: A1 is the primary and fork weight follows A1
* inactive blocks: B1 is the primary and fork weight follows B1

During the flip the subcommittees remain constant but change flip
their **primary/secondary** position.

Switch: (A1,b1) -> (A1, b2)
* active blocks: A1 is the primary and fork weight follows A1
* inactive blocks: A1 is the primary and fork weight follows A1

For **switch** transitions, on every epoch boundary, the subcommittee
is recomputed from the new **rotation seed** if the transition was
not successful, a whole epoch went without a root.

During the switch the primary remains constant.

