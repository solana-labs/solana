---
title: Optimize RocksDB Compaction for Solana BlockStore
---

This document explores RocksDB based solutions for Solana BlockStore
mentioned in issue [#16234](https://github.com/solana-labs/solana/issues/16234).

## Background
Solana uses RocksDB as the underlying storage for its blockstore.  RocksDB
is a LSM-based key value store which consists of multiple logical levels,
and data in each level is sorted by key (read amplification).  In such
leveled structure, each read hits at most one file for each level, while
all other mutable operations including writes, deletions, and merge
operations are implemented as append operations and will eventually create
more logical levels which makes the read performance worse over time.

To make reads more performant over time, RocksDB periodically reduces
the number of logical levels by running compaction in background, where
part or multiple logical levels are merged into one, which increases the
number of disk I/Os (write amplification) and storage (space amplification)
required for storing each entry.  In other words, RocksDB uses compactions
to balance [write, space, and read amplifications](https://smalldatum.blogspot.com/2015/11/read-write-space-amplification-pick-2_23.html).

As different workloads have different requirements, RocksDB makes its options
highly configerable.  However, it also means its default settings might not
be always suitable.  This document focuses on RocksDB's compaction
optimization for Solana's Blockstore.

## Problems
As mentioned in [#16234](https://github.com/solana-labs/solana/issues/16234),
there're several issues in the Solana's BlockStore which runs RocksDB with
level compaction.  Here's a quick summary of the issues:

### Long Write Stalls on Shred Insertions
Remember that RocksDB periodically runs background compactions in order to
keep the number of logical levels small in order to reach the target read
amplification.  However, when the background compactions cannot catch up
the write rate, the number of logical levels will eventually exceeds the
configured limit.  In such case, RocksDB will rate-limit / stop all writes
when it reaches soft / hard threshold.

In [#14586](https://github.com/solana-labs/solana/issues/14586), it is reported
that the write stalls in Solana's use case can be 40 minutes long.  It is also
reported in [#16234](https://github.com/solana-labs/solana/issues/16234) that
writes are also slowed-down, indicating the underlying RocksDB instance has
reach the soft limit for write stall.

### Deletions are not Processed in Time
Deletions are processed in the same way as other write operations in RocksDB,
where multiple entries associated with the same key are merged / deleted
during the background compaction.  Although the deleted entries will not
be visible from the read side right after the deletion is issued, the
deleted entries (including the original data entries and its deletion
entries) will still occupy disk storage.

TBD: explain how write-key order makes this worse

### High Write I/O from Write Amplification
In addition to write stalls, it is also observed that compactions cause
unwanted high write I/O.  With the current design where level compaction
is configured for BlockStore, it has ~30x write amplification (10x write amp
per level and assuming three levels in average).

## Current Design
Blockstore stores three types of data in RocksDB: shred data, metadata,
accounts and transactional data.  Each stores in multiple different column
families.  For shred insertions, write batches are used to combine several
shred insertions that update both shred data and metadata related column
families, while column families related to accounts and transactional are
unrelated.

In the current BlockStore, the default level compaction is used for all
column families.  As deletions are not processed in time by RocksDB,
a slot-ID based compaction filter with periodic manual compactions is used
in order to force the deletions to be processed.  While this approach
can guarantees deletions are processed for specified period of time which
mitigates the write stall issue, period manual compactions will introduce
additional write amplification.

## The Proposed Design
As all the above issues are compaction related, it can be solved with a proper
compaction style and deletion policy.  Fortunately, shred data column families,
ShredData and ShredCode, which contribute to 99% of the storage size in shred
insertion, have an unique write workload where write-keys are mostly
monotonically increasing over time.  This allows data to be persisted in sorted
order naturally without compaction, and the deletion policy can be as simple as
deleting the oldest file when the storage size reaches the cleanup trigger.

In the proposed design, we will leverage such unique property to aggressively
config RocksDB to run as few compactions as possible while offering low read
amplification with no write stalls.

### Use FIFO Compaction for Shred Data Column Families
As mentioned above, shred data column families, ShredData and ShredCode, which
contribute to 99% of the storage size in shred insertion, have an unique write
workload where write-keys are mostly monotonically increasing over time.  As a
result, after entries are flushed from memory into SST files, the keys are
naturally sorted across multiple SST files where each SST file might have
a small overlapping key range between at most two other SST files.  In other
words, files are sorted naturally from old to new, which allows us to use
the First-In-First-Out compaction, or FIFO Compaction.

FIFO Compaction actually does not compact files.  Instead, it simply deletes
the oldest files when the storage size reaches the specified threshold.  As a
result, it has a constant 1 write amplification.  In addition, as keys are
naturally sorted across multiple SST files, each read can be answered by
hitting mostly only one (or in the boundary case, two) file.  This gives us
close to 1 read amplification.  As each key is only inserted once, we have
space amplification 1.

### Use Current Settings for Metadata Column Families
The second type of the column families related to shred insertion is medadata
column families.  These metadata column families contributes ~1% of the shred
insertion data in size.  The largest metadata column family here is the Index
column family, which occupies 0.8% of the shred insertion data.

As these column families only contribute ~1% of the shred insertion data in
size, the current settings with default level compaction with compaction filter
should be good enough for now.  We can revisit later if these metadata column
families become the performance bottleneck after we've optimized the shred data
column families.

## Benefits

### No More Write Stalls
Write stall is a RocksDB's mechanism to slow-down or stop writes in order to
allow compactions to catch up in order to keep read amplification low.
Luckily, because keys in data shred column families are written in mostly
monotonically increasing order, the resulting SST files are naturally sorted
that always keeps read amplification close to 1.  As a result, there is no
need to stall writes in order to maintain the read amplification.

### Deletions are Processed in time
In FIFO compactions, deletions are happened immediately when the size of the
column family reaches the configured trigger.  As a result, deletions are
always processed in time, and we don't need to worry about whether RocksDB
picks the correct file to process the deletion as FIFO compaction always
pick the oldest one, which is the correct deletion policy for shred data.

### Low I/Os with Minimum Amplification Factors
FIFO Compaction offers constant write amplification as it does not run any
compactions in background while it usually has a large read amplification
as each read must be answered by reaching every single SST file.  However, it
is not the case in the shred data column families because SST files are naturally
sorted as write keys are inserted in mostly monotonically increasing order
without duplication.  This gives us 1 space amplification and close to 1 read
amplification.

To sum up, if no other manual compaction is issued for quickly picking up
deletions, FIFO Compaction offers the following amplification factors
in Solana's BlockStore use case:

- Write Amplification: 1 (all data is written once without compaction.)
- Read Amplification: < 1.1 (assuming each SST file has 10% overlapping key
  range with another SST file.)
- Space Amplification: 1 (same data never be written in more than one SST file,
  and no additional temporary space required for compaction.)

## Migration
Here we discuss Level to FIFO and FIFO to Level migrations:

### Level to FIFO
heoretically, FIFO compaction is the superset of all other compaction styles,
as it does not have any assumption of the LSM tree structure.  However, the
current RocksDB implementation does not offer such flexibility while it is
theoretically doable.

As the current RocksDB implementation doesn't offer such flexibility, the
best option is to extend the copy tool in the ledger tool to allow it
also specifying the destired compaction style of the output DB. This approach
also ensures the resulting FIFO compacted DB can clean up the SST files
in the correct order, as the copy tool iterates from smaller (older) slots
to bigger (latest) slots, leaving the resulting SST files generated in
the correct time order, which allows FIFO compaction to delete the oldest
data just by checking the file creation time during its clean up process.

### FIFO to Level
While one can opens a FIFO-compacted DB using level compaction, the DB will
likely to encounter long write stalls.  It is because FIFO compaction puts
all files in level 0, and write stalls trigger when the number of level-0
files exceed the limit until all the level-0 files are compacted into other
levels.

To avoid the start-up write stalls, a more efficient way to perform FIFO
to level compaction is to do a manual compaction first, then open the DB.

## Release Plan
As the migration in either way cannot not be done smoothly in place, the
release will be divided into the following steps:

* v0 - merge FIFO compaction implementation with visible args.
* v1 - visible args with a big warning stating you'll lose your ledger if you enable it
* v2 - slow-roll and monitor FIFO compaction, fix any issues.
* v3 - if needed, add migration support.

In step v1, FIFO will use a different rocksdb directory (something like
rocksdb-v2 or rocksdb-fifo) to ensure that the validator will never mix
two different formats and panic.

## Experiments
## Single Node Benchmark Results
To verify the effectiveness, I ran both 1m slots and 100m slots shred insertion
benchmarks on my n2-standard-32 GC instance (32 cores 2800MHz cpu, 128GB memory,
2048GB SSD).  Each slot contains 25 shreds, and the shreds are inserted with with
8 writers.  Here are the summary of the result:

* FIFO based validator: Shred insertion took 13450.8s, 185.8k shreds/s
* Current setting: shred insertion took 30337.2s, 82.4k shreds/s

If we further remove the write lock inside the shred insertion to allow fully
concurrent shred insertion, the proposed FIFO setting can inserts 295k shreds/s:

* FIFO + no write lock: Shred insertion took 8459.3s, 295.5k shreds/s

The possibility of enabling fully concurrent multi-writer shred insertion is
discussed in #21657.

## Results from the Mainnet-Beta
To further understand the performance, I setup two validator instances joining
the Mainnet-Beta, but one with FIFO based validator and the other is based on
the current setting.  Two validators have the same machine spec (24-core
2.8kMHz CPU, 128GB memory, 768GB SSD for blockstore, and everything else
stored in the 1024GB SSD.)  Below are the results.

### Disk Write Bytes
I first compared the disk write bytes of the SSD for blockstore of the two
instances.  This number represents how many bytes written are required in
order to store the same amount of logical data.  It also reflects the
write amplification factor of the storage.

  * FIFO based validator: ~15~20 MB/s
  * Current setting: vs 25~30 MB/s

The result shows that FIFO-based validator writes ~33% less data to perform
the same task compared to the current setting.

### Compaction Stats on Data and Coding Shred Column Family
Another data point we have is the RocksDB compaction stats, which tells us
how much resource is spent in compaction.  Below shows the compaction stats
on data and coding shreds:

 * FIFO based validator: 188.24 GB write, 1.27 MB/s write, 0.00 GB read, 0.00 MB/s read, 870.4 seconds
 * Current setting: 719.87 GB write, 4.88 MB/s write, 611.61 GB read, 4.14 MB/s read, 5782.6 seconds

The compaction stats show that FIFO based validator is 6.5x faster in
compacting data shreds and coding shreds with fewer than 1/3 disk writes.
In addition, there is no disk read involved in FIFO's compaction process.

## Summary
This documents proposes a FIFO-compaction based solution to the performance
issues of blockstore [#16234](https://github.com/solana-labs/solana/issues/16234).
It minimizes read / write / space amplification factors by leveraging the
unique property of Solana BlockStore workload where write-keys are mostly
monotonically increasing over time.  Experimental results from the single
node 100m slots insertion indicate the proposed solution can insert 185k
shred/s, which is ~2.25x faster than current design that inserts 82k shreds/s.
Experimental results from Mainnet-Beta also shows that the proposed FIFO-based
solution can achieve same task with 33% fewer disk writes compared to the
current design.
