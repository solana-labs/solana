# Enable Multi-Writers in Shred Insertion

## Problem
The current shred insertion is single-threaded which prevents us from fully
utilizing the write bandwidth of our RocksDB backed BlockStore.

The entire shred insertion is protected by an `insert_shreds_lock`, which
ensures the atomicity of the read-modify-write operations on meta-data
column families.

## Opportunities
RocksDB has an API called merge operator.  In a high level, it offers
lockless read-modify-write operation where we only write the delta
as a merge operand to the DB, and such merge operand will be used to
compute the final value by merging it with its older value.

For instance, suppose we want to read a value and write value+1 back
to RocksDB.  Instead of obtaining a write lock, reading the value,
performing value + 1, writing value + 1 back to the DB then release
the write lock, we simply write `+1` back to the DB and let such `+1`
being processed only at the read time or compaction time.  This allows
us to update an entry without the need of acquiring a write lock.

## Existing Solution
A write lock is used to perform read-modify-write during the shred insertion,
which prevents us from having multiple writers.

## Proposed Solution
To enable running multiple writers in parallel, we need to ensure the
atomicity of each shred insertion and avoid data races.  To achieve this,
both the shared in-memory structures and BlockStore access have to be
thread-safe.

For the shared in-memory structures such as BlockstoreInsertionMetrics,
we need to use a thread-safe version or only update them all at once
at the end of the function call to minimize the lock duration.

For the BlockStore, we will utilize RocksDB's merge operator to perform
read-modify-write operation to avoid data race between different writers.

Specifically, the `insert_shred()` function touches the following column
families:

### DeadSlots
DeadSlots stores [(slot, flag)](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L1212)
pairs indicating whether a particular slot can be safely ignored / deleted.
As it does not involve in any read-modify-write based on its previous state,
simply using write-batch with put operation is enough to ensure its atomicity
and thread-safety.

### ShredData / ShredCode
Both [ShredData](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L1473-L1478)
and [ShredCode](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L1274)
store data directly from the received shred and do not rely on any previous
metadata.  As a result, they are simply write operations which do not require
merge operation to ensure potential read-modify-write atomicity.

### SlotMeta
The SlotMeta column family involves in intensive read-modify-write operation
and requires converting to merge operator in order to ensure atomicity without
locking.  Here're how we can ensure thread-safety for each field:

#### [consumed](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L17-L19):
This field is [updated](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L1460-L1469)
based on the in-memory BTreeSet `data_index` by checking whether we've consumed
all shreds up to the current index.  While the consumed field does not require
additional work for converting from put operation to merge operation, we must
ensure the thread-safety of `data_index`.

#### [received](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L20-L23)
received field is updated by taking the max of the current index + 1 and the
existing value [here](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3180-L3182).
To correctly handle a potential data race where the later smaller value
overrides the older bigger value, a merge operator that implements such max
operation should be used.

#### [first_shred_timestamp](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L24-L25)
Similar to the received field, `first_shred_timestamp` can use merge operator
that includes min operation to correctly capture the `first_shred_timestamp` to
avoid the case where more than one writers are eligible for updating the
`first_shred_timestamp` field to avoid potential override in case we want to
keep this field accurate.

#### [last_index](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L26-L27)
The last_index field doesn't require merge operator to ensure its atomicity as
there should be only one shred which has its [is_last_in_slot to be true]
(https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3189-L3200).

#### [parent_slot](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L28-L29)
parent_slot is updated only from [std::u64::MAX to a valid parent slot](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3251-L3253).
A merge operator is needed to ensure we never override a valid parent slot
by u64::MAX.

#### [next_slots](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L30-L32)
next_slot is updated under [`handle_chaining`](https://github.com/yhchiang-sol/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L933)
inside [`chain_new_slot_to_prev_slot`](https://github.com/yhchiang-sol/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3543-L3550).
As we never remove entries from the next_slots field, its merge operator can be
implemented by taking the union of the new value and the existing value.

#### [is_connected](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L33-L35)
`is_connected` field is either updated based on
[the previous slot](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3535)
or via [propagation](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3469).
As `is_connected` is never set from true to false, its merge operator can be
implemented using the OR operation.

#### [completed_data_indexes](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L36-L37)
The `completed_data_indexes` remember the indexes which belonging slots
are completed inside and are updated inside
[`update_completed_data_indexes`](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3132-L3168).
Its merge operator can simply borrow `update_completed_data_indexes`.

### Orphans
Whether a slot is an orphan or not is also updated inside
[`handle_chaining_for_slot`](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L3445-L3454).

There're only two write patterns on this column family.  First,
if a parent slot is identified as an orphan, then its (slot ID, true) pair
will be put into cf::Orphans column family.  Second, when a slot is no longer
a an orphan, its orphan entry in the column family.

Because a non-orphan slot will never become orphan, the existing write pattern
to the BlockStore is already thread safe with write batch.

### ErasureMeta
Similar to SlotMeta, `insert_shreds` function also maintains a in-memory copy
of the ErasureMeta and persist them at the end of the function all at
once [here](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L942-L944).
Different from SlotMeta, the ErasureMeta does not mutate after creation:
the erasure config and set index are set during the instance
[construction](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L1068-L1072).
In addition, only the coding-shred-insertion will create the ErasureMeta,
inserting data-shred will try to load the erasure meta if any.

As a result, I feel we don't need to persist them all at once at the end,
but instead put them into write batch when they are
[constructed](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore.rs#L1068-L1072).

As for the thread-safety, since the erasure meta is only written once and
never mutate, the current write-batch can already ensure its atomicity.

### [Index](https://github.com/solana-labs/solana/blob/b57097ef18ab250aeb4ea8d3d111748ab4c41440/ledger/src/blockstore_meta.rs#L40-L46)
The Index column family maintains the mapping from a slot to its index
working set which consists of both indexes for data shreds and coding
shreds.

For both data and coding indexes, they are both updated in per shred-index
basis.  As a result, to avoid data racing, a merge operator which takes
(`DATA_OR_CODING_FLAG`, shred_index, TRUE_OR_FALSE) should be implemented
to update each field independently.

Another approach is to reimplement this ColumnFamily by using
(slot-id, DATA_OR_CODING_FLAG, index) as the key and the value
is a boolean flag indicating whether a its present or not.  This
can avoid the need of implementing a merge operator and process
merge on-the-fly during read time.

## Implementation Plan
As `insert_shred` is one of the core functions of the validator, we want to
safely and incrementally update the function and gradually minimize the lock
duration.  The implementation can be viewed into several phases.

### Phase 1: Implement and Use Merge Operator
In the first phase, we will implement the merge operator for each column
family one by one replace their `write_batch.put()` calls by
`write_batch.merge()`.  During this phase, additional unit-tests for
each merge operator will also be added.  The code logic, caching, and
read behavior of the `insert_shred` function will remain the same.
At this moment, the blockstore read and write should be thread-safe,
and its atomicity can be guaranteed by the write batch, but we will
still having the big lock as we have in-memory structure to deal with
in the second phase.

### Phase 2: Thread-Safety on Shared In-Memory Instances
In the second phase, we will start working on thread-safety around those
shared in-memory structure.  After this phase is done, the `insert_shred`
function should be thread-safe.

### Phase 3: Remove the Big Lock and Test
In the third phase, we will start wider range of testing by running
some test validators without using lock in its `insert_shred` and
identify / fix potential issues.  As we progress, we will also increase
the number of writer threads to test out its performance and reliability.

### Phase 4: Additional Optimization
Once we've passed the third phase, additonal optimization can be done
in an optional fourth phase removes possible read-modify-write logic
in the code and make the merge operator runs more efficient.
