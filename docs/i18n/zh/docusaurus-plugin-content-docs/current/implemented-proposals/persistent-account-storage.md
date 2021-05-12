---
title: 持久账户存储
---

## 持久账户存储

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. 每个验证节点都需要维护这整个集合。 Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

NVME 等持久性存储比 DDR 便宜 20 到 40 倍。 The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

## AppendVec

AppendVec 是一个数据结构，它允许随机读取与单一的纯追加写入同时进行。 增长或调整 AppendVec 的容量需要独占访问。 这是用一个原子`offset`来实现的，它在一个完成的追加结束时更新。

AppendVec 的底层内存是一个内存映射的文件。 内存映射文件允许快速的随机访问，分页由操作系统处理。

## 账户索引

账户索引的设计是为了支持所有当前分叉账户的单一索引。

```text
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Hashmap<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;
```

该索引是账户公钥的映射到分叉的映射，以及 AppendVec 中账户数据的位置。 要想获得一个特定分叉的账户版本。

```text
/// Load the account for the pubkey.
/// This function will load the account from the specified fork, falling back to the fork's parents
/// * fork - a virtual Accounts instance, keyed by Fork.  Accounts keep track of their parents with Forks,
///       the persistent store
/// * pubkey - The Account's public key.
pub fn load_slow(&self, id: Fork, pubkey: &Pubkey) -> Option<&Account>
```

通过指向存储偏移量的`AppendVecId`中的内存映射位置来满足读取。 可以返回一个没有拷贝的引用。

### 验证节点根分叉

[塔式 BFT](tower-bft.md)最终选择一个分叉作为根分叉，分叉被压扁。 被压扁的/根分叉不能回滚。

当一个分叉被压扁时，它的父账户中所有还没有出现在分叉中的账户都会通过更新索引被拉升到分叉中。 被压扁的分叉中余额为零的账户会通过更新索引从分叉中移除。

当一个账户被*压扁*导致无法访问时，可以将其垃圾回收。

有三种可能的选择。

- 维护一个 HashSet 的根分叉。 预计每秒钟创建一个。 整个树可以在以后被垃圾回收。 另外，如果每个分叉都保持一个账户的引用计数，那么在更新索引位置时，垃圾收集可能会发生。
- 从索引中删除任何修剪过的分叉。 任何剩余的比根号低的分叉都可以被认为是根号。
- 扫描索引，将任何旧的根迁移到新的索引中。 任何比新根数低的剩余分叉都可以在以后删除。

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

- 只进行追加写入的速度很快。 SSD 和 NVME，以及所有操作系统级别的内核数据结构，都允许在 PCI 或 NVMe 带宽允许的情况下以最快的速度运行追加(2,700 MB/s)。
- 每个重放和银行线程都会同时写入自己的 AppendVec。
- 每个 AppendVec 可能会被托管在一个单独的 NVMe 上。
- 每个重放和银行线程都可以并发读取所有 AppendVec，而不会阻止写入。
- 索引需要一个专属的写锁进行写入。 HashMap 更新的单线程性能在每秒 10m 左右。
- Banking 和 Replay 阶段应该使用每个 NVMe 的 32 个线程。 NVMe 使用 32 个并发读取器或写入器具有最佳性能。
