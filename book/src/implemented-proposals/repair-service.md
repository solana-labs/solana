# Repair Service

## Repair Service

The RepairService is in charge of retrieving missing shreds that failed to be delivered by primary communication protocols like Avalanche. It is in charge of managing the protocols described below in the `Repair Protocols` section below.

## Challenges:

1\) Validators can fail to receive particular shreds due to network failures

2\) Consider a scenario where blockstore contains the set of slots {1, 3, 5}. Then Blockstore receives shreds for some slot 7, where for each of the shreds b, b.parent == 6, so then the parent-child relation 6 -&gt; 7 is stored in blockstore. However, there is no way to chain these slots to any of the existing banks in Blockstore, and thus the `Shred Repair` protocol will not repair these slots. If these slots happen to be part of the main chain, this will halt replay progress on this node.

3\) Validators that find themselves behind the cluster by an entire epoch struggle/fail to catch up because they do not have a leader schedule for future epochs. If nodes were to blindly accept repair shreds in these future epochs, this exposes nodes to spam.

## Repair Protocols

The repair protocol makes best attempts to progress the forking structure of Blockstore.

The different protocol strategies to address the above challenges:

1. Shred Repair \(Addresses Challenge \#1\): This is the most basic repair protocol, with the purpose of detecting and filling "holes" in the ledger. Blockstore tracks the latest root slot. RepairService will then periodically iterate every fork in blockstore starting from the root slot, sending repair requests to validators for any missing shreds. It will send at most some `N` repair reqeusts per iteration.

   Note: Validators will only accept shreds within the current verifiable epoch \(epoch the validator has a leader schedule for\).

2. Preemptive Slot Repair \(Addresses Challenge \#2\): The goal of this protocol is to discover the chaining relationship of "orphan" slots that do not currently chain to any known fork.
   * Blockstore will track the set of "orphan" slots in a separate column family.
   * RepairService will periodically make `RequestOrphan` requests for each of the orphans in blockstore.

     `RequestOrphan(orphan)` request - `orphan` is the orphan slot that the requestor wants to know the parents of `RequestOrphan(orphan)` response - The highest shreds for each of the first `N` parents of the requested `orphan`

     On receiving the responses `p`, where `p` is some shred in a parent slot, validators will:

     * Insert an empty `SlotMeta` in blockstore for `p.slot` if it doesn't already exist.
     * If `p.slot` does exist, update the parent of `p` based on `parents`

     Note: that once these empty slots are added to blockstore, the `Shred Repair` protocol should attempt to fill those slots.

     Note: Validators will only accept responses containing shreds within the current verifiable epoch \(epoch the validator has a leader schedule for\).
3. Repairmen \(Addresses Challenge \#3\): This part of the repair protocol is the primary mechanism by which new nodes joining the cluster catch up after loading a snapshot. This protocol works in a "forward" fashion, so validators can verify every shred that they receive against a known leader schedule.

   Each validator advertises in gossip:

   * Current root
   * The set of all completed slots in the confirmed epochs \(an epoch that was calculated based on a bank &lt;= current root\) past the current root

     Observers of this gossip message with higher epochs \(repairmen\) send shreds to catch the lagging node up with the rest of the cluster. The repairmen are responsible for sending the slots within the epochs that are confrimed by the advertised `root` in gossip. The repairmen divide the responsibility of sending each of the missing slots in these epochs based on a random seed \(simple shred.index iteration by N, seeded with the repairman's node\_pubkey\). Ideally, each repairman in an N node cluster \(N nodes whose epochs are higher than that of the repairee\) sends 1/N of the missing shreds. Both data and coding shreds for missing slots are sent. Repairmen do not send shreds again to the same validator until they see the message in gossip updated, at which point they perform another iteration of this protocol.

     Gossip messages are updated every time a validator receives a complete slot within the epoch. Completed slots are detected by blockstore and sent over a channel to RepairService. It is important to note that we know that by the time a slot X is complete, the epoch schedule must exist for the epoch that contains slot X because WindowService will reject shreds for unconfirmed epochs. When a newly completed slot is detected, we also update the current root if it has changed since the last update. The root is made available to RepairService through Blockstore, which holds the latest root.

