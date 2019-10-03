# Bankless Leader

A bankless leader does the minimum amount of work to produce a valid
block.  The leader is tasked with ingress transactions, sorting and
filtering valid transactions, arranging them into entries, shredding
the entries and broadcasting the shreds.

While a validator only needs to reassemble the block and replay
execution of well formed entries.  For transactions that have
low execution costs, the leader does 3x more memory operations
before any bank execution than the validator.

## Fee Account

The [fee account](terminology.md#fee_account) pays for the
transaction to be included in the block.  The leader only needs to
validate that the fee account has the balance to pay for the
fee.

## Balance Cache

For the duration of the block, the leader maintains a temporary
balance cache for all the processed fee accounts.  The cache is
a map of pubkeys to lamports.

At the start of the block the balance cache is empty.  At the end
of the block the cache is destroyed.

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

Leaders will need to replay their block as part of the standard
replay stage operation.

## Impact on Clients

The same fee account can be reused many times in the same block
until it is used once as Credit-Debit by an instruction.

Clients that transmit a large number of transactions per second
should use a dedicated fee account that is not used as Credit-Debit
in any instruction.

Once an account fee is used as Credit-Debit, it will fail the
balance check for the remainder of the block.
