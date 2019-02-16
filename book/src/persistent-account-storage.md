# Persistent Account Storage

The set of Accounts represent the current computed state of all the transactions
that have been processed by a fullnode.  Each fullnode needs to maintain this
entire set.  Each block that is proposed by the network represents a change to
this set, and since each block is a potential rollback point the changes need to
be reversible.

Persistent storage like NVMEs are 20 to 40 times cheaper than DDR.  The problem
with persistent storage is that write and read performance is much slower then
DDR, and care must be taken in how data is read or written to.  Both reads and
writes can be split between multiple storage drives and accessed in parallel.
This design proposes a data structure that allows for concurrent reads and
concurrent writes of storage.   Writes are optimized by using an AppendVec data
structure, which allows a single writer to append while allowing access to many
concurrent readers.  The accounts index maintains a pointer to a spot where the
account was appended to for every fork, thus removing the need for explicit
check-pointing of state.

# AppendVec

AppendVec is a data structure that allows for random reads concurrent with a
single append only writer.  Grow, or resizing the capacity of the AppendVec
requires exclusive access.  This is implemented with an atomic `offset`, which
is updated at the end of a completed append.

The underlying memory for an AppendVec is a memory mapped file.  Memory mapped
files allow for fast random access, and paging is handled by the OS.

# Account Index

The account index is designed to support a single index for all the currently
forked Accounts.

```
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Vec<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;

```

The index is a map of account Pubkeys to a map of forks and the location of the
Account data in an AppendVec.  To get the latest version of an account:

```
/// Load the account for the pubkey.
/// This function will load the account from the greatest or equal to fork. 
pub fn load_slow(&self, fork: u64, pubkey: &Pubkey) -> Option<&Account>
```

The read is satisfied by pointing to a memory mapped location in the
`AppendVecId` at the stored offset.

# Append Only Writes

All the updates to Accounts occur as append only updates.  So for every account
update a new version is stored in the AppendVec.

It is possible to optimize updates within a single fork by returning a mutable
reference to an already stored account in a fork.  The Bank already tracks
concurrent access of accounts and guarantees that a write to a specific account
fork will not be concurrent with a read to an account at that fork. To support
this operation, AppendVec should implement this function:

`fn get_mut(&self, index: u64) -> &mut T`

This api allows for concurrent mutable access to a memory region at `index`.  It
relies on the Bank to guarantee exclusive access to that index.

# Garbage collection

As accounts get updated, they move to the end of the AppendVec.  Once capacity
has run out, a new AppendVec can be created and updates can be stored there.
Eventually references to an older AppendVec will disappear because all the
accounts have been updated, and the old AppendVec can be deleted.

To speed up this process, its possible to move Accounts that have not been
recently updated to the front of a new AppendVec.  This form of garbage
collection can be done without requiring exclusive locks to any of the data
structures except for the index update.

Initial implementation for garbage collection is that once all the accounts in
an AppendVec become stale versions, it gets reused. The accounts are not updated
or moved around once appended.

# Index Recovery

Each bank thread has exclusive access to the accounts during append, since the
accounts locks cannot be released until the data is committed. But there is no
explicit order of writes between the separate AppendVec files.  To create an
ordering, the index maintains an atomic write version counter.  Each append to
the AppendVec records the index write version number for that append in the
entry for the Account in the AppendVec.

To recover the index, all the AppendVec files can be read in any order, and the
latest write version for every fork should be stored in the index.

# Snapshots

To snapshot, the underlying memory mapped files in the AppendVec need to be
flushed to disk.  The index can be written out to disk as well.

# Performance

* Append only writes are fast.  SSDs and NVMEs as well as all the OS level
kernel data structures allow for appends to run as fast as PCI or NVMe bandwidth
will allow (2,700 MB/s).

* Each replay and banking thread writes concurrently to its own AppendVec.

* Each AppendVec could potentially be hosted on a separate NVMe.

* Each replay and banking thread has concurrent read access to all the
AppendVecs without blocking writes.

* Index requires an exclusive write lock for writes.  Single thread performance
for HashMap updates is on the order of 10m per second.

* Banking and Replay stages should use process 32 threads per NVMe.  NVMes have
optimal performance with 32 concurrent readers or writers.
