# Bankless Leader

A bankless leader does the minimum amount of work to produce a valid
block.  The leader is tasked with ingress transactions, sorting and
filtering valid transactions, arranging them into entries, shredding
the entries and broadcasting the shreds.  While a validator only
needs to reassemble the block and replay execution of well formed
entries.  The leader does 3x more memory operations before any bank
execution than the validator per processed transaction.

## Rationale

Normal bank operation for a spend needs to do 2 loads and 2 stores.
With this design leader just does 1 load. so 4x less account\_db
work before generating the block. The store operations are likely
to be more expensive than reads.

When replay stage starts processing the same transactions, it can
assume that PoH is valid, and that all the entries are safe for
parallel execution.  The fee accounts that have been loaded to
produce the block are likely to still be in memory, so the additional
load should be warm and the cost is likely to be amortized.

## Fee Account

The [fee account](terminology.md#fee_account) pays for the
transaction to be included in the block.  The leader only needs to
validate that the fee account has the balance to pay for the
fee.

## Balance Cache

For the duration of the leaders consecutive blocks, the leader
maintains a temporary balance cache for all the processed fee
accounts.  The cache is a map of pubkeys to lamports.

At the start of the first block the balance cache is empty.  At the
end of the last block the cache is destroyed.

The balance cache lookups must reference the same base fork for the
entire duration of the cache.  At the block boundary, the cache can
be reset along with the base fork after replay stage finishes
verifying the previous block.


## Balance Check

Prior to the balance check, the leader validates all the signatures
in the transaction.

1. Verify the accounts are not in use and BlockHash is valid.

2. Check if the fee account is present in the cache, or load the
account from accounts\_db and store the lamport balance in the
cache.

3. If the balance is less than the fee, drop the transaction.

4. Subtract the fee from the balance.

5. For all the keys in the transaction that are Credit-Debit and
are referenced by an instruction, reduce their balance to 0 in the
cache.  The account fee is declared as Credit-Debit, but as long
as it is not used in any instruction its balance will not be reduced
to 0.

## Leader Replay

Leaders will need to replay their blocks as part of the standard
replay stage operation.

## Leader Replay With Consecutive Blocks

A leader can be scheduled to produce multiple blocks in a row.  In
that scenario the leader is likely to be producing the next block
while the replay stage for the first block is playing.

When the leader finishes the replay stage it can reset the balance
cache by clearing it, and set a new fork as the base for the
cache which can become active on the next block.

## Reseting the Balance Cache

1. At the start of the block, if the balance cache is uninitialized,
set the base fork for the balance cache to be the parent of the
block and create an empty cache.

2. if the cache is initialized, check if block's parents has a new
frozen bank that is newer than the current base fork for the
balance cache.

3. if a parent newer than the cache's base fork exist, reset the
cache to the parent.

## Impact on Clients

The same fee account can be reused many times in the same block
until it is used once as Credit-Debit by an instruction.

Clients that transmit a large number of transactions per second
should use a dedicated fee account that is not used as Credit-Debit
in any instruction.

Once an account fee is used as Credit-Debit, it will fail the
balance check until the balance cache is reset.
