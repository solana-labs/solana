# Anatomy of a Transaction

Transactions encode lists of instructions that are executed sequentially, and only committed if all the instructions complete successfully. All account states are reverted upon the failure of a transaction. Each Transaction details the accounts used, including which must sign and which are credit only, a recent blockhash, the instructions, and any signatures.

## Accounts and Signatures

Each transaction explicitly lists all accounts that it needs access to. This includes accounts that are transferring tokens, accounts whose user data is being modified, and the program accounts that are being called by the instructions. Each account that is not an executable program can be marked as a requiring a signature and/or as credit only. All accounts marked as signers must have a valid signature in the transaction's list of signatures before the transaction is considered valid. Any accounts marked as credit only may only have their token value increased, and their user data is read only. Accounts are locked by the runtime, ensuring that they are not modified by a concurrent program while the transaction is running. Credit only accounts can safely be shared, so the runtime will allow multiple concurrent credit only locks on an account.

## Recent Blockhash

A Transaction includes a recent blockhash to prevent duplication and to give transactions lifetimes. Any transaction that is completely identical to a previous one is rejected, so adding a newer blockhash allows multiple transactions to repeat the exact same action. Transactions also have lifetimes that are defined by the blockhash, as any transaction whose blockhash is too old will be rejected.

## Instructions

Each instruction specifies a single program account \(which must be marked executable\), a subset of the transaction's accounts that should be passed to the program, and a data byte array instruction that is passed to the program. The program interprets the data array and operates on the accounts specified by the instructions. The program can return successfully, or with an error code. An error return causes the entire transaction to fail immediately.

