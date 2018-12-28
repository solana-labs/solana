# Entry Tree

This document proposes a change to ledger and window to support Solana's [fork
generation](fork-generation.md) behavior.

## Current Design

### Functionality of Window And Ledger

The basic responsibilities of the window and the ledger in a Solana fullnode
are:

 1. Window: serve as a temporary, RAM-backed store of blobs of the PoH chain
    for reordering and assembly into contiguous blocks to be sent to the bank
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

2. The EntryTree maintains metadata for each slot, in the `SlotMeta` struct containing:
      * `slot_index` - The index of this slot
      * `num_blocks` - The number of blocks in the slot (used for chaining to a previous slot)
      * `consumed` - The highest blob index `n`, such that for all `m < n`, there exists a blob in this slot with blob index equal to `n` (i.e. the highest consecutive blob index).
      * `received` - The highest received blob index for the slot
      * `next_slots` - A list of future slots this slot could chain to. Used when rebuilding
      the ledger to find possible fork points.
      * `consumed_ticks` - Tick height of the highest received blob (used to identify when a slot is full)
      * `is_trunk` - True iff every block from 0...slot forms a full sequence without any holes. We can derive is_trunk for each slot with the following rules. Let slot(n) be the slot with index `n`, and slot(n).contains_all_ticks() is true if the slot with index `n` has all the ticks expected for that slot. Let is_trunk(n) be the statement that "the slot(n).is_trunk is true". Then:
      
      is_trunk(0)
      is_trunk(n+1) iff (is_trunk(n) and slot(n).contains_all_ticks()

3. Chaining - When a blob for a new slot `x` arrives, we check the number of blocks (`num_blocks`) for that new slot (this information is encoded in the blob). We then know that this new slot chains to slot `x - num_blocks`.

4. Subscriptions - The EntryTree records a set of slots that have been "subscribed" to. This means entries that chain to these slots will be sent on the EntryTree channel for consumption by the ReplayStage. See the `EntryTree APIs` for details.

5. Update notifications - The EntryTree notifies listeners when slot(n).is_trunk is flipped from false to true for any `n`. 

### EntryTree APIs

The EntryTree offers a subscription based API that ReplayStage uses to ask for entries it's interested in. The entries will be sent on a channel exposed by the EntryTree. These subscription API's are as follows:
   1. `fn get_slots_since(slot_indexes: &[u64]) -> Vec<SlotMeta>`: Returns new slots connecting to any element of the list `slot_indexes`.

   2. `fn get_slot_entries(slot_index: u64, entry_start_index: usize, max_entries: Option<u64>) -> Vec<Entry>`: Returns the entry vector for the slot starting with `entry_start_index`, capping the result at `max` if `max_entries == Some(max)`, otherwise, no upper limit on the length of the return vector is imposed.

Note: Cumulatively, this means that the replay stage will now have to know when a slot is finished, and subscribe to the next slot it's interested in to get the next set of entries. Previously, the burden of chaining slots fell on the EntryTree.

### Interfacing with Bank

The bank exposes to replay stage:

 1. `prev_id`: which PoH chain it's working on as indicated by the id of the last
    entry it processed
 2. `tick_height`: the ticks in the PoH chain currently being verified by this
    bank
 3. `votes`: a stack of records that contain:

    1. `prev_ids`: what anything after this vote must chain to in PoH
    2. `tick_height`: the tick height at which this vote was cast
    3. `lockout period`: how long a chain must be observed to be in the ledger to
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
