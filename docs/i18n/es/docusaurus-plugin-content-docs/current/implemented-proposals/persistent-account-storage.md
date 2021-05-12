---
title: Persistent Account Storage
---

## Persistent Account Storage

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. Each validator needs to maintain this entire set. Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

Persistent storage like NVMEs are 20 to 40 times cheaper than DDR. The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

## AppendVec

AppendVec is a data structure that allows for random reads concurrent with a single append-only writer. Growing or resizing the capacity of the AppendVec requires exclusive access. This is implemented with an atomic `offset`, which is updated at the end of a completed append.

The underlying memory for an AppendVec is a memory-mapped file. Memory-mapped files allow for fast random access and paging is handled by the OS.

## Account Index

The account index is designed to support a single index for all the currently forked Accounts.

```text
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Hashmap<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;
```

The index is a map of account Pubkeys to a map of Forks and the location of the Account data in an AppendVec. To get the version of an account for a specific Fork:

```text
/// Load the account for the pubkey.
/// This function will load the account from the specified fork, falling back to the fork's parents
/// * fork - a virtual Accounts instance, keyed by Fork.  Accounts keep track of their parents with Forks,
///       the persistent store
/// * pubkey - The Account's public key.
pub fn load_slow(&self, id: Fork, pubkey: &Pubkey) -> Option<&Account>
```

The read is satisfied by pointing to a memory-mapped location in the `AppendVecId` at the stored offset. A reference can be returned without a copy.

### Root Forks

[Tower BFT](tower-bft.md) eventually selects a fork as a root fork and the fork is squashed. A squashed/root fork cannot be rolled back.

When a fork is squashed, all accounts in its parents not already present in the fork are pulled up into the fork by updating the indexes. Accounts with zero balance in the squashed fork are removed from fork by updating the indexes.

An account can be _garbage-collected_ when squashing makes it unreachable.

Three possible options exist:

- Maintain a HashSet of root forks. One is expected to be created every second. The entire tree can be garbage-collected later. Alternatively, if every fork keeps a reference count of accounts, garbage collection could occur any time an index location is updated.
- Remove any pruned forks from the index. Any remaining forks lower in number than the root are can be considered root.
- Scan the index, migrate any old roots into the new one. Any remaining forks lower than the new root can be deleted later.

## Garbage collection

As accounts get updated, they move to the end of the AppendVec. Once capacity has run out, a new AppendVec can be created and updates can be stored there. Eventually references to an older AppendVec will disappear because all the accounts have been updated, and the old AppendVec can be deleted.

To speed up this process, it's possible to move Accounts that have not been recently updated to the front of a new AppendVec. This form of garbage collection can be done without requiring exclusive locks to any of the data structures except for the index update.

The initial implementation for garbage collection is that once all the accounts in an AppendVec become stale versions, it gets reused. The accounts are not updated or moved around once appended.

## Index Recovery

Each bank thread has exclusive access to the accounts during append, since the accounts locks cannot be released until the data is committed. But there is no explicit order of writes between the separate AppendVec files. To create an ordering, the index maintains an atomic write version counter. Each append to the AppendVec records the index write version number for that append in the entry for the Account in the AppendVec.

To recover the index, all the AppendVec files can be read in any order, and the latest write version for every fork should be stored in the index.

## Snapshots

To snapshot, the underlying memory-mapped files in the AppendVec need to be flushed to disk. The index can be written out to disk as well.

## Performance

- Append-only writes are fast. SSDs and NVMEs, as well as all the OS level kernel data structures, allow for appends to run as fast as PCI or NVMe bandwidth will allow \(2,700 MB/s\).
- Each replay and banking thread writes concurrently to its own AppendVec.
- Each AppendVec could potentially be hosted on a separate NVMe.
- Each replay and banking thread has concurrent read access to all the AppendVecs without blocking writes.
- Index requires an exclusive write lock for writes. Single-thread performance for HashMap updates is on the order of 10m per second.
- Banking and Replay stages should use 32 threads per NVMe. NVMes have optimal performance with 32 concurrent readers or writers.
