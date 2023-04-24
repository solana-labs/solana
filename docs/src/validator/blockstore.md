---
title: Blockstore
---

After a block reaches finality, all blocks from that one on down to the genesis block form a linear chain with the familiar name blockchain. Until that point, however, the validator must maintain all potentially valid chains, called _forks_. The process by which forks naturally form as a result of leader rotation is described in [fork generation](../cluster/fork-generation.md). The _blockstore_ data structure described here is how a validator copes with those forks until blocks are finalized.

The blockstore allows a validator to record every shred it observes on the network, in any order, as long as the shred is signed by the expected leader for a given slot.

Shreds are moved to a fork-able key space the tuple of `leader slot` + `shred index` \(within the slot\). This permits the skip-list structure of the Solana protocol to be stored in its entirety, without a-priori choosing which fork to follow, which Entries to persist or when to persist them.

Repair requests for recent shreds are served out of RAM or recent files and out of deeper storage for less recent shreds, as implemented by the store backing Blockstore.

## Functionalities of Blockstore

1. Persistence: the Blockstore lives in the front of the nodes verification

   pipeline, right behind network receive and signature verification. If the

   shred received is consistent with the leader schedule \(i.e. was signed by the

   leader for the indicated slot\), it is immediately stored.

2. Repair: repair is the same as window repair above, but able to serve any

   shred that's been received. Blockstore stores shreds with signatures,

   preserving the chain of origination.

3. Forks: Blockstore supports random access of shreds, so can support a

   validator's need to rollback and replay from a Bank checkpoint.

4. Restart: with proper pruning/culling, the Blockstore can be replayed by

   ordered enumeration of entries from slot 0. The logic of the replay stage

   \(i.e. dealing with forks\) will have to be used for the most recent entries in

   the Blockstore.

## Blockstore Design

1. Entries in the Blockstore are stored as key-value pairs, where the key is the concatenated slot index and shred index for an entry, and the value is the entry data. Note shred indexes are zero-based for each slot \(i.e. they're slot-relative\).
2. The Blockstore maintains metadata for each slot, in the `SlotMeta` struct containing:

   - `slot_index` - The index of this slot
   - `num_blocks` - The number of blocks in the slot \(used for chaining to a previous slot\)
   - `consumed` - The highest shred index `n`, such that for all `m < n`, there exists a shred in this slot with shred index equal to `n` \(i.e. the highest consecutive shred index\).
   - `received` - The highest received shred index for the slot
   - `next_slots` - A list of future slots this slot could chain to. Used when rebuilding

     the ledger to find possible fork points.

   - `last_index` - The index of the shred that is flagged as the last shred for this slot. This flag on a shred will be set by the leader for a slot when they are transmitting the last shred for a slot.
   - `is_connected` - True iff every block from 0...slot forms a full sequence without any holes. We can derive is_connected for each slot with the following rules. Let slot\(n\) be the slot with index `n`, and slot\(n\).is_full\(\) is true if the slot with index `n` has all the ticks expected for that slot. Let is_connected\(n\) be the statement that "the slot\(n\).is_connected is true". Then:

     is_connected\(0\) is_connected\(n+1\) iff \(is_connected\(n\) and slot\(n\).is_full\(\)

3. Chaining - When a shred for a new slot `x` arrives, we check the number of blocks \(`num_blocks`\) for that new slot \(this information is encoded in the shred\). We then know that this new slot chains to slot `x - num_blocks`.
4. Subscriptions - The Blockstore records a set of slots that have been "subscribed" to. This means entries that chain to these slots will be sent on the Blockstore channel for consumption by the ReplayStage. See the `Blockstore APIs` for details.
5. Update notifications - The Blockstore notifies listeners when slot\(n\).is_connected is flipped from false to true for any `n`.

## Blockstore APIs

The Blockstore offers a subscription based API that ReplayStage uses to ask for entries it's interested in. These subscription API's are as follows:

1. `fn get_slots_since(slots: &[u64]) -> Result<HashMap<u64, Vec<u64>>>`: Returns slots that are connected to any of the elements of `slots`. This method enables the discovery of new children slots.

2. `fn get_slot_entries(slot: Slot, shred_start_index: u64) -> Result<Vec<Entry>>`: For the specified `slot`, return a vector of the available, contiguous entries starting from `shred_start_index`. Shreds are fragments of serialized entries so the conversion from entry index to shred index is not one-to-one. However, there is a similar function `get_slot_entries_with_shred_info()` that returns the number of shreds that comprise the returned entry vector. This allows a caller to track progress through the slot.

Note: Cumulatively, this means that the replay stage will now have to know when a slot is finished, and subscribe to the next slot it's interested in to get the next set of entries. Previously, the burden of chaining slots fell on the Blockstore.

## Interfacing with Bank

The bank exposes to replay stage:

1. `prev_hash`: which PoH chain it's working on as indicated by the hash of the last entry it processed

2. `tick_height`: the ticks in the PoH chain currently being verified by this bank

3. `votes`: a stack of records that contains:
    * `prev_hashes`: what anything after this vote must chain to in PoH
    * `tick_height`: the tick height at which this vote was cast
    * `lockout period`: how long a chain must be observed to be in the ledger to be able to be chained below this vote

Replay stage uses Blockstore APIs to find the longest chain of entries it can hang off a previous vote. If that chain of entries does not hang off the latest vote, the replay stage rolls back the bank to that vote and replays the chain from there.

## Pruning Blockstore

Once Blockstore entries are old enough, representing all the possible forks becomes less useful, perhaps even problematic for replay upon restart. Once a validator's votes have reached max lockout, however, any Blockstore contents that are not on the PoH chain for that vote for can be pruned, expunged.
