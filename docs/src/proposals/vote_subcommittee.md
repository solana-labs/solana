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
is voting for

* primary subcommittee: The half of the voting subcommittee that
is scheduled for its second epoch.

* secondary subcommittee: The half of the voting subcommittee that
is scheduled for its first epoch.

* rotation block: The last block in the previous epoch that was
rooted by 2/3+ of both primary and secondary super-majorities.

* rotation seed: The seed used to generate the random sample of nodes.
It's computed as `slow_hash(rotation block bank_hash)`


* `slow_hash`: repeated sha256 for rounds equal to 1ms on modern
sha-ni hardware.

### Rules

1. Subcommittees are scheduled for 2 epochs, and rotate in a staggered
pattern.

```
voting epoch: 0  1  2  3  4
             A1 A1 A2 A2 A3
                B1 B1 B2 B2
```

2. First epoch the subcommittee is **secondary**. Epoch 2, (A2,B1)
is the **voting subcommittee** and B1 is **primary**, A2 is
**secondary**.

3. Second epoch the subcommittee is **primary**

4. The new **secondary** subcommittee is determined by the **rotation
seed**

5. **rotation seed** is based on the **rotation block**

6. **rotation block** was rooted by the current **primary** and the
previous **secondary** AND **primary**

7. There can only be one **rotation block** because the new **primary**
rooted the **rotation block** and all paths that cross the epoch
boundary can only point to the same **rotation block** or a lockout
violation has occurred.

For switching proofs:

1. If the primary committee is switching from the current epoch,
it can only use its own votes for switching proofs

2. If the primary committee is switching from a previous epoch in
which it was the secondary, it can use its own votes from when it
was the secondary and the previous epoch primary votes on the primary
fork. Switching proofs can't mix votes from both subcommittees.

3. The secondary committee can use any votes from the primary and
itself to switch.

### Safety Violations

An Optimistic Confirmation violation occurs if a voting subcommittee
optimistically confirms a slot that wasn't rooted by the following
voting subcommittee.

A Safety violation occurs if a subcommittee roots a slot that wasn't
rooted by the following subcommittee.

All full nodes including RPCs should halt as soon as a safety
violation is detected.

### Subcommittee Rotation

Voting Subcommittee is scheduled for a voting epoch, and is itself
composed of 2 subcommittees, primary and secondary, scheduled for
two epochs each in a staggered rotation.

```
A1 A1 A2 A2 A3
   B1 B1 B2 B2
```

The first epoch the subcommittee runs, it is considered **secondary**.
On the second epoch, it is considered **primary**.

For a new secondary subcommittee to start, it must start with a
block that is a descendant of a **rotation block** that was confirmed
by both super-majorities in the previous epoch. The **rotation seed**
is used to derive the new secondary subcommittee.

The epoch can be as short as 32 slots, since a slot needs to be
rooted.  But to make rotation more reliable it makes sense to have
the epoch be at least 100 slots long, so the probability of a slot
being rooted is high enough for rotation to nearly always occur.

If there is no rooted block during the epoch, the same voting
subcommittee continues. Primary subcommittee remains the same until
the rotation.

### Subcommittee Selection

A stake weighted shuffle from the **rotation seed** is used to pick
the next secondary subcommittee. A slow hash is necessary to ensure
that grinding the **rotation block** bankhash is not practical. A
random sample of 200 nodes picked with a stake weighed shuffle.
Assuming 1/3 are faulty, probability of 133 or greater faulty nodes
is 1:10^20. A 1ms hash would require grinding for 10^9 years with
1 billion cores.

### Threshold Switching

Secondary subcommittee can use the primary's votes as a switching
threshold proof. The primary can only use its own votes for switching
proofs.

When secondary uses to switch forks using the primary votes, the
fork that its switching from must be from the secondary epoch.

```
A1 A1 A2 A2 A3
   B1 B1 B2 B2
```

Last block of epoch (A1,B1), X, A1 is primary and has optimistically
confirmed slot X. B1 in this example was locked out did not
optimistically confirm slot X. On the next slot, X+1, B1 is now
primary. B1 may use votes from A1 on slot X to switch forks.  The
fork form which B1 is switching from must be from the epoch when
B1 was still secondary, earlier than slot X.

### Safety

```
A1 A1 A2 A2 A3
   B1 B1 B2 B2
```

To switch subcommittees both currently active primary and secondary
must confirm the same slot with a 2/3+ super-majority. When voting
subcommittee switches from (A1,B1) to (A2,B1), B1 must have rooted
the same fork as A1, and (A2,B1) must include the same fork, or a
lockout violation has occurred, or the network has stalled. At the
start of the epoch with (A2,B1), B1 is now the primary and A2 is
the secondary.

### Optimistic Confirmation Safety

A slot is optimistically confirmed if and only if both subcommittees
confirm it with 2/3+ majority.

When a subcommittee was **secondary**, it can use the **primary**
votes to switch forks.  But since both **primary** and **secondary**
confirmed a slot, it is not possible to construct a switching proof.
Switching proofs must contain votes only from one subcommittee, the
votes can't be mixed.

Voting subcommittee (A1,B1) confirms the last slot `X` of its
epoch, (A2,B1) take over on the same epoch. B1 must root `X`, or B1
has committed a threshold switch violation. Since B1 is primary,
it can only use its own votes for a switching proof.

### Block producers

All block producing validators even those outside of the subcommittee
schedule should follow the primary subcommittee fork weight. They
should be internally running as **secondary** and generate switching
proofs based on observed **primary** votes.
