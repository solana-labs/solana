# Credit-Only Accounts

This design covers the handling of credit-only and credit-debit accounts in the
[runtime](runtime.md).  Accounts already distinguish themselves as credit-only or
credit-debit based on the program ID specified by the transaction's instruction.
Programs must treat accounts that are not owned by them as credit-only.

To identify credit-only accounts by program id would require the account to be
fetched and loaded from disk.  This operation is expensive, and while it is
occurring, the runtime would have to reject any transactions referencing the same
account.

The proposal introduces a `num_readonly_accounts` field to the transaction
structure, and removes the `program_ids` dedicated vector for program accounts.

This design doesn't change the runtime transaction processing rules.
Programs still can't write or spend accounts that they do not own, but it
allows the runtime to optimistically take the correct lock for each account
specified in the transaction before loading the accounts from storage.

Accounts selected as credit-debit by the transaction can still be treated as
credit-only by the instructions.

## Runtime handling

credit-only accounts have the following properties:

* Can be deposited into:  Deposits can be implemented as a simple `atomic_add`.
* read-only access to account data.

Instructions that debit or modify the credit-only account data will fail.

## Account Lock Optimizations

The Accounts module keeps track of current locked accounts in the runtime,
which separates credit-only accounts from the credit-debit accounts.  The credit-only
accounts can be cached in memory and shared between all the threads executing
transactions.

The current runtime can't predict whether an account is credit-only or credit-debit when
the transaction account keys are locked at the start of the transaction
processing pipeline.  Accounts referenced by the transaction have not been
loaded from the disk yet.

An ideal design would cache the credit-only accounts while they are referenced by
any transaction moving through the runtime, and release the cache when the last
transaction exits the runtime.

## Credit-only accounts and read-only account data

Credit-only account data can be treated as read-only. Credit-debit
account data is treated as read-write.

## Transaction changes

To enable the possibility of caching accounts only while they are in the
runtime, the Transaction structure should be changed in the following way:

* `program_ids: Vec<Pubkey>` - This vector is removed.  Program keys can be
placed at the end of the `account_keys` vector within the `num_readonly_accounts`
number set to the number of programs.

* `num_readonly_accounts: u8` - The number of keys from the **end** of the
transaction's `account_keys` array that is credit-only.

The following possible accounts are present in an transaction:

* paying account
* RW accounts
* R accounts
* Program IDs

The paying account must be credit-debit, and program IDs must be credit-only.  The
first account in the `account_keys` array is always the account that pays for
the transaction fee, therefore it cannot be credit-only.  For these reasons the
credit-only accounts are all grouped together at the end of the `account_keys`
vector.  Counting credit-only accounts from the end allow for the default `0`
value to still be functionally correct, since a transaction will succeed with
all credit-debit accounts.

Since accounts can only appear once in the transaction's `account_keys` array,
an account can only be credit-only or credit-debit in a single transaction, not
both.  The runtime treats a transaction as one atomic unit of execution. If any
instruction needs credit-debit access to an account, a copy needs to be made. The
write lock is held for the entire time the transaction is being processed by
the runtime.

## Starvation

Read locks for credit-only accounts can keep the runtime from executing
transactions requesting a write lock to a credit-debit account.

When a request for a write lock is made while a read lock is open, the
transaction requesting the write lock should be cached.  Upon closing the read
lock, the pending transactions can be pushed through the runtime.

While a pending write transaction exists, any additional read lock requests for
that account should fail.  It follows that any other write lock requests will also 
fail.  Currently, clients must retransmit when a transaction fails because of 
a pending transaction.  This approach would mimic that behavior as closely as
possible while preventing write starvation.

## Program execution with credit-only accounts

Before handing off the accounts to program execution, the runtime can mark each
account in each instruction as a credit-only account.  The credit-only accounts can
be passed as references without an extra copy.  The transaction will abort on a
write to credit-only.

An alternative is to detect writes to credit-only accounts and fail the
transactions before commit.

## Alternative design

This design attempts to cache a credit-only account after loading without the use
of a transaction-specified credit-only accounts list.  Instead, the credit-only
accounts are held in a reference-counted table inside the runtime as the
transactions are processed.

1. Transaction accounts are locked.
    a. If the account is present in the ‘credit-only' table, the TX does not fail.
       The pending state for this TX is marked NeedReadLock.
2. Transaction accounts are loaded.
    a. Transaction accounts that are credit-only increase their reference
       count in the `credit-only` table.
    b. Transaction accounts that need a write lock and are present in the
       `credit-only` table fail.
3. Transaction accounts are unlocked.
    a. Decrement the `credit-only` lock table reference count; remove if its 0
    b. Remove from the `lock` set if the account is not in the `credit-only`
       table.

The downside with this approach is that if the `lock` set mutex is released
between lock and load to allow better pipelining of transactions, a request for
a credit-only account may fail. Therefore, this approach is not suitable for
treating programs as credit-only accounts.

Holding the accounts lock mutex while fetching the account from disk would
potentially have a significant performance hit on the runtime. Fetching from
disk is expected to be slow, but can be parallelized between multiple disks.
