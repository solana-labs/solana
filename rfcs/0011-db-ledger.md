# Unified Window and Ledger

This RFC describes a change to ledger and window to support Solana's [consensus](0002-consensus.md) algorithm.

## Current Design

### Functionality of Window And Ledger

The basic responsibilities of the window and the ledger in a Solana fullnode are:

 1. Window: serve as a temporary, RAM-backed store of blobs of the PoH chain for re-ordering and assembly into contiguous blocks to be sent to the bank for verification.
 2. Window: serve as a RAM-backed repair facility for other validator nodes, which may query the network for as-yet unreceived blobs.
 3. Ledger: provide disk-based storage of the PoH chain in case of node restart.
 4. Ledger: provide disk-backed repair facility for when the (smaller) RAM-backed window doesn't cover the repair request.

The window is at the front of a validator node's processing pipeline, blobs are received, cached, re-ordered before being deserialized into Entries, passed to the bank for verification, and finally on to the ledger, which is at the back of a validator node's pipeline.

The window holds blobs (the over-the-air format, serialized Entries, one-per-blob).  The ledger holds serialized Entries without any blob information.

### Limitations

#### One-dimensional key space

The window and the ledger are indexed by ledger height, which is number of Entries ever generated in the PoH chain until the current blob.  This limitation prevents the window and the ledger from storing the overlapping histories possible in Solana's consensus protocol.

#### Limited caching

The window is a circular buffer.  It cannot accept blobs that are farther in the future than the window is currently working.  If a blob arrives that is too far ahead, it is dropped and will subsequently need to be repaired, incurring further delay for the node.

#### Loss of blob signatures

Because the blob signatures are stripped before being stored by the ledger, repair requests served from the ledger can't be verified to the original leader.

#### Rollback and checkpoint, switching forks, separate functions

The window and the ledger can't handle replay of alternate forks.  Once a Blob has passed through the window, it's in the past.  The replicate stage of a validator will need to roll back to a previous checkpoint and decode an alternate set of Blobs to the Bank.  The separated and one-way nature of window and ledger makes this hard.

## New Design

### Unified Window and Ledger: DbLedger

A unified window and ledger would allow a validator to record every blob it observes on the network, in any order, as long as the blob is consistent with the network's leader schedule.

Blobs will be moved to a fork-able key space the tuple of `leader slot` + `blob index` (within the slot).  This permits the skip-list structure of the Solana protocol to be stored in its entirety, without a-priori choosing which fork to follow, which Entries to persist or when to persist them.

Repair requests for recent blobs are served out of RAM or recent files and out of deeper storage for less recent blobs, as implemented by the store backing DbLedger.

### Functionalities of DbLedger (and hereafter "Ledger")

1. Persistence: the ledger lives in the front of the nodes verification pipeline, right behind network receive and signature verification.  If the blob received is consistent with the leader schedule (i.e. was signed by the leader for the indicated slot), it is immediately stored.
2. Repair: repair is the same as window repair above, but able to serve any blob that's been received. The ledger stores blobs with signatures, preserving the chain of origination.
3. Forks: the ledger supports random access of blobs, so can support a validator's need to rollback and replay from a Bank checkpoint.
4. Restart: the

### Interfacing with Bank

The bank exposes to replicate_stage:

 1. prev_id: which PoH chain it's working on as indicated by the id of the last entry it processed
 2. tick_height: the ticks in the PoH chain currently being verified by this bank
 4. votes: a stack of records that contain
     a. prev_ids: what anything after this vote must chain to in PoH
     b. tick height: the tick_height at which this vote was cast
     c. lockout period: how long a chain must be observed to be in the ledger to be able to be chained below this vote

### Culling

Once ledger entries are old enough, representing the possible forks becomes less useful, perhaps even problematic for replay upon restart.  Once a validator's votes have reached max lockout, however, any ledger contents that are not on the PoH chain voted for can be expunged.

Replicator nodes will be responsible for storing really old ledger contents, and validators need only persist their bank periodically.
