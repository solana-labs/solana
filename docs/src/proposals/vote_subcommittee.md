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

* primary subcommittee: The half of the voting subcommittee that
is scheduled for its second epoch.

* secondary subcommittee: The half of the voting subcommittee that
is scheduled for its first epoch.

* subcommittee seed: The seed used to generate the random sample of
nodes. `slow_hash(penultimate snapshot hash, voting epoch number)`

* super root: The min of the primary and secondary supermajority
roots.

* SRI - super root increase: When the super root is increased by
any number of super roots between the parent and child bank.

### Subcommittee Rotation

```

a0 a1 A1 A1 a1 a2 A2 A2 a2 a3
B1 B1 b1 b2 B2 B2 b2 b3 B3 B3
```

Voting subcommittee is composed of a **primary** and **secondary**
committies. The rotation occurs after N super root increases. The
child bank that detects the Nth **SRI** is what activates the rotation.

Network should be stable with N = 1. Since the epoch boundary depends
on the number of **SRIs**, it is not a fixed number of slots or a
fixed number of roots.  At N=1 the primary rotation is likley to
take 1 root, and the secondary rotation likely to take many roots
as the **secondary** catches up with the **primary**.

The subcommittee rotation is computed before any vote processing
as part of Bank creation.

#### Primary Rotation

In this rotation, the current **primary** and **secondary** flip.
Heaviest fork is always determined by the **primary**.

#### Secondary rotation

In this rotation, the current **primary** stays constant and
**secondary** rotates to a new randomly sampled subcommittee.
Heaviest fork is always determined by the **primary**.

### Optimistically Confirmed Safety

In the **primary rotation** phase, BOTH **primary** and **secondary**
must have 2/3+ votes on the same fork.

### Optimistically Confirmed Liveness

Only the primary votes maybe used for switching proofs.
