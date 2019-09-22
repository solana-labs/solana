# Anatomy of a Transaction

Transactions encode lists of instructions that are executed sequentially, and only committed if all the instructions complete successfully. All account updates are reverted upon the failure of a transaction. Each transaction details the accounts used, including which must sign and which are credit only, a recent blockhash, the instructions, and any signatures.

## Accounts and Signatures

Each transaction explicitly lists all account public keys referenced by the transaction's instructions. A subset of those public keys are each accompanied by a transaction signature. Those signatures signal on-chain programs that the account holder has authorized the transaction. Typically, the program uses the authorization to permit debiting the account or modifying its data.

The transaction also marks some accounts as _credit-only accounts_. The runtime permits credit-only accounts to be credited concurrently. If a program attempts to debit a credit-only account or modify its account data, the transaction is rejected by the runtime.

## Recent Blockhash

A Transaction includes a recent blockhash to prevent duplication and to give transactions lifetimes. Any transaction that is completely identical to a previous one is rejected, so adding a newer blockhash allows multiple transactions to repeat the exact same action. Transactions also have lifetimes that are defined by the blockhash, as any transaction whose blockhash is too old will be rejected.

## Instructions

Each instruction specifies a single program account \(which must be marked executable\), a subset of the transaction's accounts that should be passed to the program, and a data byte array instruction that is passed to the program. The program interprets the data array and operates on the accounts specified by the instructions. The program can return successfully, or with an error code. An error return causes the entire transaction to fail immediately.

