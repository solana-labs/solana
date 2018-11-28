# Forking Ledger

This RFC describes a change to ledger and window to support Solana's consensus algorithm.

## Current Design

### Functionality of Window And Ledger

The basic responsibilities of the Window and the Ledger in a Solana fullnode are:

 1. Window: serve as a temporary, RAM-backed store of Blobs of the PoH chain for re-ordering and assembly into contiguous blocks to be sent to the Bank for verification.
 2. Window: serve as a RAM-backed repair facility for other validator nodes, which may query the network for as-yet unreceived blobs.
 3. Ledger: provide disk-based storage of the PoH chain in case of node restart.
 4. Ledger: provide disk-backed repair facility for when the (smaller) RAM-backed Window doesn't cover the repair request.

The Window is at the front of a validator node's processing pipeline, Blobs are received, cached, re-ordered before being deserialized into Entries, passed to the Bank for verification, and finally on to the Ledger, which is at the back of a validator node's pipeline.

Window holds Blobs (the over-the-air format, serialized Entries, one-per-blob).  The Ledger holds serialized Entries without any Blob information.

### Limitations

#### Limited key space
The Window and the Ledger are indexed by ledger height, which is number of blobs ever generated in the PoH chain until the current blob.  This limitation prevents the Window and the Ledger from storing the overlapping histories possible in Solana's consensus protocol.

#### Limited caching

The Window is a circular buffer.  It cannot accept Blobs that are farther in the future than the Window is currently working.  If a Blob arrives that is too far ahead, it is dropped and will subsequently need to be repaired, incurring further delay for the node.

#### Loss of Blob signatures

Because the Blob signatures are stripped before being stored by the Ledger, repair requests served from the Ledger can't be verified to the original leader.

#### Rollback and checkpoint, switching forks, separate functions

The Window and the Ledger can't handle replay of alternate forks.  Once a Blob has passed through the Window, it's in the past.  The replicate stage of a validator will need to roll back to a previous checkpoint and decode an alternate set of Blobs to the Bank.  The separated and one-way nature of Window and Ledger makes this hard.

## New Design

### Unified Window and Ledger: DbLedger

A unified Window and Ledger would allow a validator to record every Blob it observes on the network, in any order, as long as the Blob is consistent with the network's leader schedule.

Blobs will be moved to a fork-able key space the tuple of leader slot + Blob index within that slot.  This permits the skip-list structure of the Solana protocol to be represented in its entirety, without a-priori choosing which fork to follow, which Entries to persist.

Repair requests for recent Blobs are served out of RAM or recent files and out of deeper storage for less recent Blobs.

### Functionality of DBLedger

on a persistent key-value store
that has a limited amount of space.  This presents a difficulty for validators that are meaningfully behind the rest of the network, because

To date, these functions were separated into 2 modules (Window and Ledger).  These modules made the assumption that PoH had only one possible chain, which .  They also stored their data in

Validators on the network are responsible for observing, verifying, and voting on the network's shared state.  Updates to the state are structured like a skip list as described in [Consensus](0002-consensus.md).

Validators pick a fork to play through their Bank to verify and vote upon.  Validators optimistically pick the longest fork available to them that does not violate their lockout periods from prior votes.

## Data Flow

### Interfacing with Bank

### Culling

### Repair
