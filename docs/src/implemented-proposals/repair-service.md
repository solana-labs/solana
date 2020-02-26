# Repair Service

## Repair Service

The RepairService is in charge of retrieving missing shreds that failed to be
delivered by primary communication protocols like Turbine. It is in charge of
managing the protocols described below in the `Repair Protocols` section below.

## Challenges:

1\) Validators can fail to receive particular shreds due to network failures

2\) Consider a scenario where blockstore contains the set of slots {1, 3, 5}.
Then Blockstore receives shreds for some slot 7, where for each of the shreds
b, b.parent == 6, so then the parent-child relation 6 -&gt; 7 is stored in
blockstore. However, there is no way to chain these slots to any of the
existing banks in Blockstore, and thus the `Shred Repair` protocol will not
repair these slots. If these slots happen to be part of the main chain, this
will halt replay progress on this node.

## Repair-related primitives
Epoch Slots:
   Each validator advertises separately on gossip thhe various parts of an
   `Epoch Slots`:
   * The `stash`: An epoch-long compressed set of all completed slots.
   * The `cache`: The Run-length Encoding (RLE) of the latest `N` completed
     slots starting from some some slot `M`, where `N` is the number of slots
     that will fit in an MTU-sized packet.

   `Epoch Slots` in gossip are updated every time a validator receives a
   complete slot within the epoch. Completed slots are detected by blockstore
   and sent over a channel to RepairService. It is important to note that we
   know that by the time a slot `X` is complete, the epoch schedule must exist
   for the epoch that contains slot `X` because WindowService will reject
   shreds for unconfirmed epochs.

   Every `N/2` completed slots, the oldest `N/2` slots are moved from the
   `cache` into the `stash`. The base value `M` for the RLE should also
   be updated.
   
## Repair Request Protocols

The repair protocol makes best attempts to progress the forking structure of
Blockstore.

The different protocol strategies to address the above challenges:

1. Shred Repair \(Addresses Challenge \#1\): This is the most basic repair
protocol, with the purpose of detecting and filling "holes" in the ledger.
Blockstore tracks the latest root slot. RepairService will then periodically
iterate every fork in blockstore starting from the root slot, sending repair
requests to validators for any missing shreds. It will send at most some `N`
repair reqeusts per iteration. Shred repair should prioritize repairing
forks based on the leader's fork weight. Validators should only send repair
requests to validators who have marked that slot as completed in their
EpochSlots. Validators should prioritize repairing shreds in each slot
that they are responsible for retransmitting through turbine. Validators can
compute which shreds they are responsible for retransmitting because the
seed for turbine is based on leader id, slot, and shred index.

   Note: Validators will only accept shreds within the current verifiable
   epoch \(epoch the validator has a leader schedule for\).

2. Preemptive Slot Repair \(Addresses Challenge \#2\): The goal of this
protocol is to discover the chaining relationship of "orphan" slots that do not
currently chain to any known fork. Shred repair should prioritize repairing
orphan slots based on the leader's fork weight.
   * Blockstore will track the set of "orphan" slots in a separate column family.
   * RepairService will periodically make `Orphan` requests for each of
   the orphans in blockstore.

     `Orphan(orphan)` request - `orphan` is the orphan slot that the
     requestor wants to know the parents of `Orphan(orphan)` response -
     The highest shreds for each of the first `N` parents of the requested
     `orphan`

     On receiving the responses `p`, where `p` is some shred in a parent slot,
     validators will:

     * Insert an empty `SlotMeta` in blockstore for `p.slot` if it doesn't
     already exist.
     * If `p.slot` does exist, update the parent of `p` based on `parents`

     Note: that once these empty slots are added to blockstore, the
     `Shred Repair` protocol should attempt to fill those slots.

     Note: Validators will only accept responses containing shreds within the
     current verifiable epoch \(epoch the validator has a leader schedule
     for\).

Validators should try to send orphan requests to validators who have marked that
orphan as completed in their EpochSlots. If no such validators exist, then
randomly select a validator in a stake-weighted fashion.

## Repair Response Protocol

When a validator receives a request for a shred `S`, they respond with the
shred if they have it. 

When a validator receives a shred through a repair response, they check
`EpochSlots` to see if <= `1/3` of the network has marked this slot as
completed. If so, they resubmit this shred through its associated turbine
path, but only if this validator has not retransmitted this shred before.

