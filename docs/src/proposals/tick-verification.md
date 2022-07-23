---
title: Tick Verification
---

This design the criteria and validation of ticks in a slot. It also describes
error handling and slashing conditions encompassing how the system handles
transmissions that do not meet these requirements.

# Slot structure

Each slot must contain an expected `ticks_per_slot` number of ticks. The last
shred in a slot must contain only the entirety of the last tick, and nothing
else. The leader must also mark this shred containing the last tick with the
`LAST_SHRED_IN_SLOT` flag. Between ticks, there must be `hashes_per_tick`
number of hashes.

# Handling bad transmissions

Malicious transmissions `T` are handled in two ways:

1. If a leader can generate some erronenous transmission `T` and also some
   alternate transmission `T'` for the same slot without violating any slashing
   rules for duplicate transmissions (for instance if `T'` is a subset of `T`),
   then the cluster must handle the possibility of both transmissions being live.

Thus this means we cannot mark the erronenous transmission `T` as dead because
the cluster may have reached consensus on `T'`. These cases necessitate a
slashing proof to punish this bad behavior.

2. Otherwise, we can simply mark the slot as dead and not playable. A slashing
   proof may or may not be necessary depending on feasibility.

# Blockstore receiving shreds

When blockstore receives a new shred `s`, there are two cases:

1. `s` is marked as `LAST_SHRED_IN_SLOT`, then check if there exists a shred
   `s'` in blockstore for that slot where `s'.index > s.index` If so, together `s`
   and `s'` constitute a slashing proof.

2. Blockstore has already received a shred `s'` marked as `LAST_SHRED_IN_SLOT`
   with index `i`. If `s.index > i`, then together `s` and `s'`constitute a
   slashing proof. In this case, blockstore will also not insert `s`.

3. Duplicate shreds for the same index are ignored. Non-duplicate shreds for
   the same index are a slashable condition. Details for this case are covered
   in the `Leader Duplicate Block Slashing` section.

# Replaying and validating ticks

1. Replay stage replays entries from blockstore, keeping track of the number of
   ticks it has seen per slot, and verifying there are `hashes_per_tick` number of
   hashes between ticks. After the tick from this last shred has been played,
   replay stage then checks the total number of ticks.

Failure scenario 1: If ever there are two consecutive ticks between which the
number of hashes is `!= hashes_per_tick`, mark this slot as dead.

Failure scenario 2: If the number of ticks != `ticks_per_slot`, mark slot as
dead.

Failure scenario 3: If the number of ticks reaches `ticks_per_slot`, but we still
haven't seen the `LAST_SHRED_IN_SLOT`, mark this slot as dead.

2. When ReplayStage reaches a shred marked as the last shred, it checks if this
   last shred is a tick.

Failure scenario: If the signed shred with the `LAST_SHRED_IN_SLOT` flag cannot
be deserialized into a tick (either fails to deserialize or deserializes into
an entry), mark this slot as dead.
