---
title: Vote Subcommittee
---

## Problem

Each additional validator adds 1 vote per slot, increasing the
message and compute load on the network.

## Solution Overview

Allow a sampled set of validators to vote on that block as opposed
to all the validators, and achieve probabilistically similar liveness
properties as if all the validators vote.

## Detailed Solution

The following sections provide more details of the design.

### Definitions

* Voting Subcommittee: the set of nodes currently voting on blocks

* Voting epoch: the number of roots that the voting subcommittee
is voting for. This is separate from the leader schedule epoch.

* primary subcommittee: The half of the voting subcommittee that
is scheduled for its second epoch.

* secondary subcommittee: The half of the voting subcommittee that
is scheduled for its first epoch.

* subcomittee seed: The seed used to generate the random sample of
nodes. `slow_hash(penultimate snapshot hash, voting epoch start slot)`

### Subcommittee Rotation

```

a0 a1 A1 A1 a1 a2 A2 A2 a2 a3
B1 B1 b1 b2 B2 B2 b2 b3 B3 B3
```

From a high level a voting subcommittee is composed of a **primary**
and **secondary**, in general the votes from the **primary**
subcommittee should be the what all the block producers are using
for fork weight.

Rotation is activated on a block when N 2/3+ roots are achieved by
of the previous subcommittees.  The network will obseve some blocks
with N roots, and some with N-1 roots. For each kind of rotation
this document will show that N-1 blocks always converges to N without
loss of liveness or breaking safety assumptions of optimistically
confirmed blocks.

#### Primary rotation

Starts with block that have N-1 roots of (A1, b1) and will transition
to (a1, B1) at the Nth block. 

Roots must contain BOTH, **primary** and **secondary** subcommittee
2/3+ votes.

Block producers will follow A1 fork weight for any blocks proposed
with N-1 roots.

For any forks with N-1 roots, b1 may use A1's votes to
switch forks.

On blocks with N roots, block producers will follow B1s votes, and
on those blocks the network is in the **secondary rotation** mode.

#### Secondary rotation

Starts with block that have N-1 roots of (a1, B1) and will transition
to (a2, B1) at the Nth block

Block producers will follow B1 fork weight for any blocks proposed
with N-1 roots or N roots.

Roots may contain ONLY **primary** subcommittee 2/3+ votes.

