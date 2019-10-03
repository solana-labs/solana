# Bankless Leader

A bankless leader does the minimum amount of work to produce a valid
block.  The leader is tasked with ingress transactions, sorting and
filtering valid transactions, arranging them into entries, shreding
the entries and broadcasting the shreds.  While a validator only
needs to reassemble the block and replay it.


## Execution Key Balance Check

The [execution key](terminology.md#execution_key) pays for the
transaction to be included in the block.  The leader only needs to
validate that the execution key has the balance to pay for the
transaction fee.

1. Verify the accounts are not in use and BankHash is valid.

2. check if the key is present in the cache, or load the key from
accounts\_db and store the key and the last lamport balance in the
cache.

3. if the balance is less than the fee, drop the transaction.

4. subtract the fee from the balance

5. for all the keys in the transaction that are used as Credit-Debit,
mark their balance as 0 by storing the balance in the cache.

## Leader Replay

Leaders will need to replay their block as part of the standard
replay stage operation.

## Impact on Clients

The same execution key can be reused many times in the same block
until it is used once as a Credit-Debit key.

Clients that transmit a large number of transactions per second
should use a dedicated execution key that is not used as Credit-Debit
in any transactions.
