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

1. `default_transaction_queue` - A max-heap `BinaryHeap<Transaction>` that tracks all pending transactions.
The priority in the heap is the additional fee of the transaction. Transactions are added to this queue
from sigverify before the leader slot begins.
2. `all_transaction_queues` - A `VecDeque<BinaryHeap<Transaction>>` that tracks all pending queues of work.
Some pending queues have higher priority than others (as will be explained later in the `Handling Completion Signals from BankingStage Threads` section below). The list is ordered in priority from highest to lowest priority. On
initialization, `all_transaction_queues[0] = default_transaction_queue`.
3. `locked_accounts` - A `HashMap<LockedPubkey, usize>` that tracks the set of accounts needed to execute the
current set of transactions scheduled/sent to banking threads. Accounts are added to this set
*before* being sent to BankingStage threads. The `usize` represents a refcount, because multiple read
accounts could be grabbed.`LockedPubkey` is defined as:
```
enum LockedPubkey {
    Read(Pubkey),
    Write(Pubkey),
}
```
4. `blocked_transactions` -  A `HashMap<Signature, Rc<BlockedTransactionsQueue>>` keyed by
transaction signature, and maps to a `BlockedTransactionsQueue` defined as:
```
/// Represents a heap of transactions that cannot be scheduled because they
/// would take locks on accounts needed by a higher paying transaction
struct BlockedTransactionsQueue {
    // The higher priority transaction blocking all the other transactions in
    // `blocked_transactions` below
    highest_priority_blocked_transaction: Transaction,
    other_blocked_transactions: BinaryHeap<Transaction>
}
```
5. `blocked_transaction_queues_by_accounts` - A `HashMap<Pubkey, Rc<BlockedTransactionsQueue>>` keyed by
account keys.

#### Algorithm (Main Loop):

Assume `N` BankingStage threads:

The scheduler will run for each banking thread a function `find_next_highest_transaction()`:

1. Pop off the highest priority transaction `next_highest_transaction` from `self.all_transaction_queues[0]`.
If ``self.all_transaction_queues[0]` is empty, pop off the first deque item and continue.

2. Let `transaction_accounts` be the set of `LockedPubkey` keys needed by
`next_highest_transaction`. We run the following:

```
    for account_key in transaction_accounts {
        // Check if the `LockedPubkey` conflicts with any key in the `locked_accounts` set, which
        // would indicate a transaction using this account with a conflicting lock is already
        // running
        if self.locked_accounts.is_conflicting(account_key) {
            return Conflict;
        }

        // Check if any higher fee transaction has already reserved this account. This prevents
        // lower fee transactions from starving higher fee transactions.
        if self.blocked_transaction_queues_by_accounts.contains_key(account_key) {
            return Conflict;
        }
        return NoConflict;
    }
```

3. In the case of a `NoConflict` we run:

```
    for account_key in transaction_accounts {
        self.locked_accounts.insert_reference(account_key.key());
    }

    banking_thread_channel.send(next_highest_transaction);
```

4. In the case of a `Conflict` we run:

```
for locked_account_key in transaction_accounts {
    let account_key = locked_account_key.key()
    let blocked_transaction_entry = self.blocked_transaction_queues_by_accounts.entry(account_key);
    match blocked_transaction_entry {
        Occupied(existing_blocked_transaction) => {
            // If there is already a set of transactions blocked on this account, add
            // this transaction to the priority queue.
            existing_blocked_transaction.insert_transaction(next_highest_transaction);
        }

        Vacant(vacant_entry) => {
            // Create a new queue blocked on this transaction
            let new_blocked_transaction_queue =
                Rc::new(BlockedTransactionsQueue {
                    highest_priority_blocked_transaction: next_highest_transaction,
                    other_blocked_transactions: BinaryHeap::new(),
                });
            // Insert into the hashmap for this `account_key`
            vacant_entry.insert(new_blocked_transaction_queue.clone());
            // Insert into the `blocked_transactions` hashmap to indicate this set of transactions
            // is blocked by `next_highest_transaction`
            self.blocked_transactions.insert(
                next_highest_transaction.signature(),
                new_blocked_transaction_queue
            );
        }
    }
}
```

5. Run until all `N` BankingStage threads have been sent `processing_batch` transactions (i.e. hit step 3 above).

#### Banking Threads
1. Banking threads maintain a queue of transactions sent to them by the scheduler, sorted by priority.
2. Because the scheduler has guaranteed that there are no locking conflicts, the banking thread can process
some `M` of these transactions at a time and pack them into entries

#### Handling Completion Signals from BankingStage Threads

Outside of the main loop above, we rely on BankingThreads threads signaling us they've finished their
task to schedule the next transactions.

1. Once a BankingStage thread finishes processing a batch of transactions `completed_transactions_batch` ,
it sends the `completed_transactions_batch` back to the scheduler via the same channel to signal of completion.

2. Upon receiving this signal, the BankingStage thread processes the locked accounts
`transaction_accounts` for each `completed_transaction` in `completed_transactions_batch`:
```
let mut unlocked_accounts = vec![];
// First remove all the locks from the tracking list
for locked_account in transaction_accounts {
    if self.locked_accounts.remove_reference(locked_account) {
        unlocked_accounts.push(locked_account.key());
    }
}

// Check if freeing up these accounts has now allowed any new
// blocked transactions to run
for account_key in unlocked_accounts {
    if let Some(blocked_transaction_queue) = self.blocked_transaction_queues_by_accounts.get(account_key) {
        // Check if the transaction blocking this queue can be run now, thereby unblocking this queue
        if blocked_transaction_queue.highest_priority_blocked_transaction.can_get_locks() {
            // Schedule the transaction to the banking thread
            banking_thread_channel.send(blocked_transaction_queue.highest_priority_blocked_transaction);

            return;
        }
    }

    // If no higher priority transactions were unblocked, continue scheduling from the main queue,
    // described in the main loop section above
    find_next_highest_transaction();
}
```

3. Check if the finished transaction was the blocking transaction for any queue:

```
if let Some(blocked_transaction_queue) = self.blocked_transactions.get(completed_transaction.signature) {
    // Now push the rest of the queue to the head of `all_transaction_queues`, since we know
    // everything in this blocked queue must be of higher priority, (since they were
    // added to this queue earlier, this means they must have been peopped off the main
    // `transaction_accounts` queue earlier, hence higher priority)
    self.all_transaction_queues.push_front(blocked_transaction_queue.other_blocked_transactions);
    self.blocked_transactions.remove(completed_transaction.signature);
}
```