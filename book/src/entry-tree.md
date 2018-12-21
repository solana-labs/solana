# Entry Tree

This document proposes a change to ledger and window to support Solana's [fork
generation](fork-generation.md) behavior.

## Current Design

### Functionality of Window And Ledger

The basic responsibilities of the window and the ledger in a Solana fullnode
are:

 1. Window: serve as a temporary, RAM-backed store of blobs of the PoH chain
    for re-ordering and assembly into contiguous blocks to be sent to the bank
    for verification.
 2. Window: serve as a RAM-backed repair facility for other validator nodes,
    which may query the network for as-yet unreceived blobs.
 3. Ledger: provide disk-based storage of the PoH chain in case of node
    restart.
 4. Ledger: provide disk-backed repair facility for when the (smaller)
    RAM-backed window doesn't cover the repair request.

The window is at the front of a validator node's processing pipeline, blobs are
received, cached, re-ordered before being deserialized into Entries, passed to
the bank for verification, and finally on to the ledger, which is at the back
of a validator node's pipeline.

The window holds blobs (the over-the-air format, serialized Entries,
one-per-blob).  The ledger holds serialized Entries without any blob
information.

### Limitations

#### One-dimensional key space

The window and the ledger are indexed by ledger height, which is number of
Entries ever generated in the PoH chain until the current blob.  This
limitation prevents the window and the ledger from storing the overlapping
histories possible in Solana's consensus protocol.

#### Limited caching

The window is a circular buffer.  It cannot accept blobs that are farther in
the future than the window is currently working.  If a blob arrives that is too
far ahead, it is dropped and will subsequently need to be repaired, incurring
further delay for the node.

#### Loss of blob signatures

Because the blob signatures are stripped before being stored by the ledger,
repair requests served from the ledger can't be verified to the original
leader.

#### Rollback and checkpoint, switching forks, separate functions

The window and the ledger can't handle replay of alternate forks.  Once a Blob
has passed through the window, it's in the past.  The replay stage of a
validator will need to roll back to a previous checkpoint and decode an
alternate set of Blobs to the Bank.  The separated and one-way nature of window
and ledger makes this hard.

## New Design

A unified window and ledger allows a validator to record every blob it observes
on the network, in any order, as long as the blob is consistent with the
network's leader schedule.

Blobs are moved to a fork-able key space the tuple of `leader slot` + `blob
index` (within the slot).  This permits the skip-list structure of the Solana
protocol to be stored in its entirety, without a-priori choosing which fork to
follow, which Entries to persist or when to persist them.

Repair requests for recent blobs are served out of RAM or recent files and out
of deeper storage for less recent blobs, as implemented by the store backing
EntryTree.

### Functionalities of EntryTree

1. Persistence: the EntryTree lives in the front of the nodes verification
   pipeline, right behind network receive and signature verification.  If the
blob received is consistent with the leader schedule (i.e. was signed by the
leader for the indicated slot), it is immediately stored.
2. Repair: repair is the same as window repair above, but able to serve any
   blob that's been received. EntryTree stores blobs with signatures,
preserving the chain of origination.
3. Forks: EntryTree supports random access of blobs, so can support a
   validator's need to rollback and replay from a Bank checkpoint.
4. Restart: with proper pruning/culling, the EntryTree can be replayed by
   ordered enumeration of entries from slot 0.  The logic of the replay stage
(i.e. dealing with forks) will have to be used for the most recent entries in
the EntryTree.

### EntryTree Design

1. Entries in the EntryTree are stored as key-value pairs, where the key is the concatenated
slot index and blob index for an entry, and the value is the entry data. Note blob indexes are zero-based for each slot (i.e. they're slot-relative).

2. The EntryTree maintains metadata for each slot, containing:
      1. `slot_index` - The index of this slot
      2. `num_blocks` - The number of blocks in the slot (used for chaining to a previous slot)
      3. `consumed` - The highest blob index `n`, such that for all `m < n`, there exists a blob in this slot with blob index equal to `n` (i.e. the highest consecutive blob index).
      4. `received` - The highest received blob index for the slot
      5. `next_slots` - A list of future slots this slot could chain to. Used when rebuilding
      the ledger to find possible fork points.
      6. `previous_slot` - The previous slot index that this slot chains to, used to identify
      whether the entries in this slot are of interest to any subscriptions
      7. `highest_tick` - Tick height of the highest received blob (used to identify when a slot is full)

3. Chaining - When a blob for a new slot `x` arrives, we check the number of blocks (`num_blocks`) for that new slot (this information is encoded in the blob). We then know that this new slot chains to slot `x - num_blocks`.

4. Subscriptions - The EntryTree records a set of slots that have been "subscribed" to. This means entries that chain to these slots will be sent on the EntryTree channel for consumption by the ReplayStage. See the `EntryTree APIs` for details.

### EntryTree APIs

The EntryTree offers a subscription based API that ReplayStage uses to ask for entries it's interested in. The entries will be sent on a channel exposed by the EntryTree. These subscription API's are as follows:
   1. `fn subscribe(slot_index: u64) -> ()`: Let `s` be the slot such that `s.slot_index == slot_index`. This `subscribe(slot_index)` function call, tells the EntryTree to return all entries `e` such that:
   
   `(slot(e).slot_index \in slot.next_slots) && (e.blob_index <= slot(e).consumed)`

   where `slot(e)` is the slot that contains `e`.

   As new entries are receieved and added to the EntryTree, they will also be continually sent to the ReplayStage as long as they satisfy the above condition. This means that for every newly received entry `e`, if 'e.blob_index' causes `slot(e)` to increment `slot(e).consumed` (i.e. a new consecutive chain of entries is formed due to inserting `e` and plugging the hole in the slot at `e.blob_index`), then the EntryTree will iterate over all slots in the `subscriptions` set and check if `slot(e)` chains to any of those slots. If so,it will send the next consecutive chain of entries to the ReplayStage.

   2. `fn unsubscribe(slot_index: u64) -> ()`: Unsubscribe from a slot.

Note: Cumulatively, this means that the ReplayStage will now have to know when a slot is finished, and subscribe to the next slot it's interested in to get the next set of entries. Previously, the burden of chaining slots fell on the EntryTree.

### Interfacing with Bank

The bank exposes to replay stage:

 1. prev_id: which PoH chain it's working on as indicated by the id of the last
    entry it processed
 2. tick_height: the ticks in the PoH chain currently being verified by this
    bank
 3. votes: a stack of records that contain

    1. prev_ids: what anything after this vote must chain to in PoH
    2. tick height: the tick_height at which this vote was cast
    3. lockout period: how long a chain must be observed to be in the ledger to
       be able to be chained below this vote

Replay stage uses EntryTree APIs to find the longest chain of entries it can
hang off a previous vote.  If that chain of entries does not hang off the
latest vote, the replay stage rolls back the bank to that vote and replays the
chain from there.

### Pruning EntryTree

Once EntryTree entries are old enough, representing all the possible forks
becomes less useful, perhaps even problematic for replay upon restart.  Once a
validator's votes have reached max lockout, however, any EntryTree contents
that are not on the PoH chain for that vote for can be pruned, expunged.

Replicator nodes will be responsible for storing really old ledger contents,
and validators need only persist their bank periodically.
