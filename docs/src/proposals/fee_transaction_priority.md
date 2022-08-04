---
title: Fee Transaction Priority
---

Additional fees were introduced to transactions as a method to allow users to bid for priority for
their transactions in the leader's queue.

The fee priority of a transaction `T` can then be defined as `F(T)`, where `F(T)` is the "fee-per
compute-unit", calculated by:

`(additional_fee + base_fee) / requested_compute_units`

To ensure users get fair priority based on their fee, the proposed scheduler for the leader must
guarantee that given `T1` and `T2` in the pending queue, and `F(T1) > F(T2)`:

1. `T1` should be considered for processing before `T2`


### Transaction Pipeline

Pipeline:
1. Sigverify
2. Scheduler
3. BankingStage threads

Transactions from sigverify are connected via a channel to the scheduler. The scheduler maintains
`N` bi-directional channels with the `N` BankingStage threads, implemented as a pair of two
unidirectional channels.

The scheduler's job is to run an algorithm in which it determines how to schedule transactions
received from sigverify to the `N` BankingStage threads. A transaction is scheduled to be executed
on a particular BankingStage thread by sending that transaction to the thread via its associated
channel.

Once a BankingStage thread finishes processing a transaction `T` , it sends the `T` back
to the scheduler via the same channel to signal of completion.

### Scheduler Implementation

The scheduler is the most complex piece of the above pipeline, its implementation is made up of a
few pieces. In cases where the scheduler is receiving a large number of transactions, it may be necessary
to have a separate thread for deserializing and inserting transactions into the scheduler's structs.

#### Components of the `Scheduler`:

1. `pending_transactions` - A max-heap `BinaryHeap<Transaction>` that tracks all pending transactions that are not currently known to be blocked.
2. `account_status` - A map `HashMap<Pubkey, AccountScheduleStatus>` that tracks the number of unscheduled reads and writes, as well as the current lock status per account.
    The `AccountScheduleStatus` is defined as:
    ```
        struct AccountScheduleStatus {
            /// The number of unscheduled read transactions
            reads: u64,
            /// The number of unscheduled write transactions
            writes: u64,
            /// Current scheduled lock on the account
            scheduled_lock: AccountLock,
        }
    ```
    where `AccountLock` is defined as:
    ```
        struct AccountLock {
            /// Set of batch ids that the account is scheduled with read-only locks on
            read_batches: HashSet<TransactionBatchId>,
            /// Set of batch ids that the account is scheduled with write locks on
            write_batches: HashSet<TransactionBatchId>,
        }
    ```
3. `blocked_transactions_by_batch_id` - A map `HashMap<TransactionBatchId, HashSet<Transaction>>` that stores the set of blocked transactions by batch id.
4. `transaction_batches` - A map `HashMap<TransactionBatchId, TransactionBatch>` that tracks transaction batches by id.
5. `in_progress_batches` - A set `HashSet<TransactionBatchId>` of batches the scheduler is currently building (unscheduled).
6. `execution_thread_stats` - A vector `Vec<ExecutionThreadStats>` that tracks stats of each execution thread, where `ExecutionThreadStats` is defined by:
    ```
        struct ExecutionThreadStats {
            /// Currently queued compute-units
            queued_compute_units: usize,
            /// Currently queued number of transactions
            queued_transactions: usize,
            /// Currently queued batch ids
            queued_batches: VecDeque<TransactionBatchId>,
        }
    ```
A `TransactionBatch` is defined by:
```
    struct TransactionBatch {
        /// Has the transaction been sent
        scheduled: bool,
        /// Timestamp of the batch starting to be built
        start_time: Instant,
        /// Identifier
        id: TransactionBatchId,
        /// Number of transactions scheduled (only > 0 after send)
        num_transactions: usize,
        /// Transactions (only valid before send)
        transactions: Vec<TransactionRef>,
        /// Locked Accounts and Kind Set (only built on send)
        account_locks: HashMap<Pubkey, AccountLockKind>,
        /// Thread it is scheduled on
        execution_thread_index: usize,
    }
```

#### Incoming Packets:
The packet handling thread will deserialize incoming packets from sigverify, and insert the transactions into the `pending_transactions` queue, and counts in the `account_status` map.


#### Algorithm (Main Loop):

1. If `in_progress_batches` is empty, do nothing since we have no batches to build.
2. pop the highest priority transaction from `pending_transactions`, and try to schedule it.
3. check for any transaction batch(es) that conflict with the transaction
   1. if there are conflicting batches across threads, mark the transaction as blocked by the conflicting batches
   2. if there are conflicting batches on a single thread, but no in progress batches on that thread, mark the transaction as blocked by the conflicting batches
   3. if there are conflicting batches on a single thread, and there is an in progress batch, add the transaction to the in progress batch
   4. if there are no conflicting batches, add the transaction to the in progress batch with the lowest thread index
4. If a transaction is added to a batch, add the `TransactionBatchId` to `account_locks`
5. If the transaction batch we scheduled to is full, either by hitting the max number of transacitons, or max CU limit, we send out the batch:
   1. Loop through transactions in the batch, and construct `account_locks`
   2. Build the vec of transactions to send to the execution thread
   3. Update `execution_thread_stats` for the new batch
6. If a transaction batch was sent, potentially create a new in progress batch:
   1. we want to keep a constant queue of batches provided to each thread
   2. ideally, we have 2 batches queued up for each execution thread

#### Banking Threads
1. Banking threads maintain a queue of transaction batches sent to them by the scheduler.
2. Because the scheduler has guaranteed that there are no locking conflicts, the banking thread can process
some `M` of these transactions at a time and pack them into entries

#### Handling Completion Signals from BankingStage Threads

Outside of the main loop above, we rely on BankingThreads threads signaling us they've finished their
task to schedule the next transactions.

1. Once a BankingStage thread finishes processing a batch of transactions `completed_transactions_batch`,
it sends the `completed_transactions` back to the scheduler via the same channel to signal of completion.
2. Upon receiving this signal, the Scheduler thread checks if the batch at the front of the execution thread's queue is complete.
3. If the batch is complete, we remove the transaction batch from tracking, unlock accounts, and potentially create a new batch.
   1. transactions from `blocked_transactions_by_batch_id` are pushed back into `pending_transactions`
4. If the batch is not completed, we decrement `num_transactions` on the batch so we can check for completion