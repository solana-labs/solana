# Bankless Leader

A bankless leader does the minimum amount of work to produce a valid
block.  The leader is tasked with ingress transactions, sorting and
filtering valid transactions, arranging them into entries, shredding
the entries and broadcasting the shreds.

While a validator only needs to reassemble the block and replay
execution of well formed entries.  For transactions that have
low execution costs, the leader does 3x more memory operations
before any bank execution than the validator.

## Execution Key

The [execution key](terminology.md#execution_key) pays for the
transaction to be included in the block.  The leader only needs to
validate that the execution key has the balance to pay for the
fee.

## Balance Cache

For the duration of the block, the leader maintains a temporary
balance cache for all the processed execution keys.  The cache is
a map of pubkeys to lamports.

At the start of the block the balance cache is empty.  At the end
of the block the cache is destroyed.

## Balance Check

Prior to the balance check, the leader validates all the signatures
in the transaction.

1. Verify the accounts are not in use and BlockHash is valid.

2. Check if the key is present in the cache, or load the key from
accounts\_db and store the key and the last lamport balance in the
cache.

3. If the balance is less than the fee, drop the transaction.

4. Subtract the fee from the balance.

5. For all the keys in the transaction that are Credit-Debit and
are referenced by an instruction, mark their balance as 0 in the
cache.  The execution key can be declared as Credit-Debit as long
as it is not used in any instruction.

## Leader Replay

Leaders will need to replay their block as part of the standard
replay stage operation.

## Impact on Clients

The same execution key can be reused many times in the same block
until it is used once as a Credit-Debit key.

Clients that transmit a large number of transactions per second
should use a dedicated execution key that is not used as Credit-Debit
in any instruction.

Once a execution key is used as Credit-Debit, it will fail the
balance check for the remainder of the block.
