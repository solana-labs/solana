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
2. If `T1` cannot be processed before `T2` because there's already a transaction currently being
processed that contends on an account `A`, then `T2` should not be scheduled if it would grab
any account locks needed by `T1`. This prevents lower fee transactions like `T2` from starving
higher paying transactions like `T1`.


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
few pieces. Note for now, all these pieces are maintained by the single scheduler thread to avoid
locking complexity.

#### Components of the `Scheduler`:

1. `pending_transactions` - A max-heap `BinaryHeap<Transaction>` that tracks all pending transactions that are not known to be blocked.
2. `transactions_by_account` - A map `HashMap<Pubkey, AccountTransactionQueue>` that tracks all pending and blocked transactions by account key.
   The `AccountTransactionQueue` is defined as:
   ```
   class AccountTransactionQueue {
       /// Tree of read transactions on the account ordered by fee-priority
       pub reads: RBTree<Transaction>,
       /// Tree of write transactions on the account ordered by fee-priority
       pub writes: RBTree<Trasaction>,
       /// The type, count, and minimum priority transaction that is currently scheduled for execution
       pub scheduled_lock: Option<AccountLock>,
   }
   ```
   and `AccountLock` is defined as:
   ```
       enum AccountLock {
           Read(usize, Transaction),
           Write(Transaction),
       }
   ```
   where the `Transaction` stored in the `AccountLock` will be the minimum priority transaction currently scheduled. The `Read` variant will also store the number
   of read transactions.
3. `blocked_transactions` - A map `HashMap<Signature, Vec<Transaction>>` that maps from the blocking transaction's `Signature` to the transactions it is blocking.
This is lazily evaluated only at the time a transaction is scheduled, or a transaction completes execution.

#### Incoming Transactions:

When the Scheduler receives transactions from SigVerify, the Scheduler's components need to be updated to reflect the new priority.
For each transaction `transaction` the Scheduler receives we run the following:
```
    for account in transaction.accounts {
        let mut account_queue = self.transactions_by_account.entry(account.key()).or_default();
        match account {
            Read(pubkey) => account_queue.reads.insert(transaction),
            Write(pubkey) => account_queue.writes.insert(transaction),
        };
    }

    self.pending_transactions.insert(transaction);
```
where `transaction.accounts` is a set of `LockedPubkey`:
```
    enum LockedPubkey {
        Read(Pubkey),
        Write(Pubkey),
    }
```
We will not update `blocked_transactions` at this point, because a lower priority, but still blocking, transaction could come in
before we try scheduling this transaction.

#### Algorithm (Main Loop):

Assume `N` BankingStage threads:

The scheduler will run for each banking thread a function `schedule_next_highest_transaction()`:
1. We run the following:

```
    // Loop through pending transactions until we find one that is not blocked
    // or run out of pending transactions
    while let Some(next_highest_transaction) in self.pending_transactions.pop() {
        let mut min_blocking_transaction = None;
        for account_key in next_highest_transaction.accounts.keys() {
            find_min_priority_blocking_transaction_for_account(account_key, &mut min_blocking_transaction);
        }

        match min_blocking_transaction {
            Some(blocking_transaction) => {
                // Mark this transaction as blocked
                self.blocking_transactions.entry(blocking_transaction.signature).or_default().insert(next_highest_transaction);
            },
            None => {
                // Lock accounts and send to the BankingStage thread
                for account in transaction.accounts {
                    // Updates the `scheduled_lock` field in this account's queue.
                    // * Read transactions will increment the count, and store the minimum priority transaction.
                    // * Write transactions will store the transaction
                    self.update_scheduled_transactions_for_account(account, transaction.priority);
                }
                banking_thread_channel.send(transaction);
                return;
            },
        }
    }
```
where
```
    fn find_min_priority_blocking_transaction_for_account(account: &LockedPubkey, min_blocking_transaction: &mut Option<Transaction>) -> Option<Transaction> {
        self.transactions_by_account.get(account.key).map(|account_transaction_queue| {
            // Get the lowest priority write transaction the blocks this transaction (writes will always block)
            if let Some(blocking_transaction) = account_transaction_queue.writes.upper_bound(transaction.priority) {
                min_blocking_transaction = min_blocking_transaction.map_or(Some(blocking_transaction), |tx| tx.min(blocking_transaction));
            }

            // If this transaction is a writing transaction, get the minimum read that blocks it
            if account.is_write() && let Some(blocking_transaction) = account_transaction_queue.reads.upper_bound(transaction.priority) {
                min_blocking_transaction = min_blocking_transaction.map_or(Some(blocking_transaction), |tx| tx.min(blocking_transaction));
            }

            // Finally, we need to check the currently scheduled transaction lock for this account
            // This is necessary because this transaction may have higher priority than transactions we scheduled
            // before this transaction came in. While these transactions are still in the trees, they won't be returned
            // by our calls to `upper_bound`.
            //
            // For read transactions, we won't be blocked by other scheduled reads, only writes.
            // For write transactions, we are blocked by either read transactions or write transactions.
            match account_transaction_queue.scheduled {
                Some(AccountLock(Read(_, scheduled_transaction))) => {
                    if account.is_write()  {
                        min_blocking_transaction = min_blocking_transaction.map_or(Some(scheduled_transaction), |tx| tx.min(scheduled_transaction));
                    }
                }
                Some(AccountLock(Write(scheduled_transaction))) => {
                    min_blocking_transaction = min_blocking_transaction.map_or(Some(scheduled_transaction), |tx| tx.min(scheduled_transaction));
                }
            }

            min_blocking_transaction
        })
    }
```
3. Run until all `N` BankingStage threads have been sent `processing_batch` transactions (i.e. hit step 3 above).

#### Banking Threads
1. Banking threads maintain a queue of transactions sent to them by the scheduler, sorted by priority.
2. Because the scheduler has guaranteed that there are no locking conflicts, the banking thread can process
some `M` of these transactions at a time and pack them into entries

#### Handling Completion Signals from BankingStage Threads

Outside of the main loop above, we rely on BankingThreads threads signaling us they've finished their
task to schedule the next transactions.

1. Once a BankingStage thread finishes processing a batch of transactions `completed_transactions_batch` ,
it sends the `completed_transactions_batch` back to the scheduler via the same channel to signal of completion.
2. Upon receiving this signal, the Scheduler thread processes the locked accounts
`transaction_accounts` for each `completed_transaction` in `completed_transactions_batch`:
```
    for account in transaction_accounts {
        let mut account_transaction_queue = self.transactions_by_account.get(account.key()).unwrap();

        // Removes the transaction from the appriopriate tree (read or write).
        // Updates `scheduled_lock`:
        //  - For reads, decrement the count. If 0, we set `scheduled_lock` to None.
        //  - For writes, set `scheduled_lock` to None.
        account_transaction_queue.remove_account_lock(account);
    }
```
3. Check if the finished transaction was the blocking transaction for any queue:

```
if let Some(blocked_transactions) = self.blocked_transactions.remove(completed_transaction.signature) {
    // Move all blocked transactions back into our pending queue
    self.pending_transactions.extend(blocked_transactions.into_iter());
}
```