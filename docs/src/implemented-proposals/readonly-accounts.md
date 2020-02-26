# Read-Only Accounts

This design covers the handling of readonly and writable accounts in the [runtime](../validator/runtime.md). Multiple transactions that modify the same account must be processed serially so that they are always replayed in the same order. Otherwise, this could introduce non-determinism to the ledger. Some transactions, however, only need to read, and not modify, the data in particular accounts. Multiple transactions that only read the same account can be processed in parallel, since replay order does not matter, providing a performance benefit.

In order to identify readonly accounts, the transaction MessageHeader structure contains `num_readonly_signed_accounts` and `num_readonly_unsigned_accounts`. Instruction `program_ids` are included in the account vector as readonly, unsigned accounts, since executable accounts likewise cannot be modified during instruction processing.

## Runtime handling

Runtime transaction processing rules need to be updated slightly. Programs still can't write or spend accounts that they do not own. But new runtime rules ensure that readonly accounts cannot be modified, even by the programs that own them.

Readonly accounts have the following property:

* Read-only access to all account fields, including lamports (cannot be credited or debited), and account data

Instructions that credit, debit, or modify the readonly account will fail.

## Account Lock Optimizations

The Accounts module keeps track of current locked accounts in the runtime, which separates readonly accounts from the writable accounts. The default account lock gives an account the "writable" designation, and can only be accessed by one processing thread at one time. Readonly accounts are locked by a separate mechanism, allowing for parallel reads.

Although not yet implemented, readonly accounts could be cached in memory and shared between all the threads executing transactions. An ideal design would hold this cache while a readonly account is referenced by any transaction moving through the runtime, and release the cache when the last transaction exits the runtime.

Readonly accounts could also be passed into the processor as references, saving an extra copy.
