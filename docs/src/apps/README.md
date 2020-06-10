# Programming Model

An _app_ interacts with a Solana cluster by sending it _transactions_ with one or more _instructions_. The Solana _runtime_ passes those instructions to _programs_ deployed by app developers beforehand. An instruction might, for example, tell a program to transfer _lamports_ from one _account_ to another or create an interactive contract that governs how lamports are transferred. Instructions are executed sequentially and atomically for each transaction. If any instruction is invalid, all account changes in the transaction are discarded.

### Accounts and Signatures

Each transaction explicitly lists all account public keys referenced by the transaction's instructions. A subset of those public keys are each accompanied by a transaction signature. Those signatures signal on-chain programs that the account holder has authorized the transaction. Typically, the program uses the authorization to permit debiting the account or modifying its data.

The transaction also marks some accounts as _read-only accounts_. The runtime permits read-only accounts to be read concurrently. If a program attempts to modify a read-only account, the transaction is rejected by the runtime.

### Recent Blockhash

A transaction includes a recent blockhash to prevent duplication and to give transactions lifetimes. Any transaction that is completely identical to a previous one is rejected, so adding a newer blockhash allows multiple transactions to repeat the exact same action. Transactions also have lifetimes that are defined by the blockhash, as any transaction whose blockhash is too old will be rejected.

### Instructions

Each instruction specifies a single program account \(which must be marked executable\), a subset of the transaction's accounts that should be passed to the program, and a data byte array instruction that is passed to the program. The program interprets the data array and operates on the accounts specified by the instructions. The program can return successfully, or with an error code. An error return causes the entire transaction to fail immediately.

## Deploying Programs to a Cluster

![SDK tools](../.gitbook/assets/sdk-tools.svg)

As shown in the diagram above, a program author creates a program and compiles it to an ELF shared object containing BPF bytecode and uploads it to the Solana cluster with a special _deploy_ transaction. The cluster makes it available to clients via a _program ID_. The program ID is a _address_ specified when deploying and is used to reference the program in subsequent transactions.

A program may be written in any programming language that can target the Berkley Packet Filter \(BPF\) safe execution environment. The Solana SDK offers the best support for C/C++ and Rust programs, which are compiled to BPF using the [LLVM compiler infrastructure](https://llvm.org).

## Storing State between Transactions

If the program needs to store state between transactions, it does so using _accounts_. Accounts are similar to files in operating systems such as Linux. Like a file, an account may hold arbitrary data and that data persists beyond the lifetime of a program. Also like a file, an account includes metadata that tells the runtime who is allowed to access the data and how.

Unlike a file, the account includes metadata for the lifetime of the file. That lifetime is expressed in "tokens", which is a number of fractional native tokens, called _lamports_. Accounts are held in validator memory and pay ["rent"](rent.md) to stay there. Each validator periodically scans all accounts and collects rent. Any account that drops to zero lamports is purged.

In the same way that a Linux user uses a path to look up a file, a Solana client uses an _address_ to look up an account. The address is usually a 256-bit public key. To create an account with a public-key address, the client generates a _keypair_ and registers its public key using the `CreateAccount` instruction with preallocated fixed storage size in bytes. In fact, the account address can be an arbitrary 32 bytes, and there is a mechanism for advanced users to create derived addresses (`CreateAccountWithSeed`). Addresses are presented in Base58 encoding on user interfaces.

## Ownership of Accounts and Assignment to Programs

The created account is initialized to be _owned_ by a built-in program called the System program and is called a _system account_ aptly. An account includes "owner" metadata. The owner is a program ID. The runtime grants the program write access to the account if its ID matches the owner. For the case of the System program, the runtime allows clients to transfer lamports and importantly _assign_ account ownership, meaning changing owner to different program ID. If an account is not owned by a program, the program is only permitted to read its data and credit the account.

Also, if an account is marked "executable" in metadata, it will only be used by a _loader_ to run programs. For example, a BPF-compiled program is marked executable and loaded by the BPF loader when executing its transactions. No program is allowed to modify the contents of an executable account once deployed.

## Runtime Capability of Programs on Accounts

The runtime only permits the owner program to debit the account or modify its data. The program then defines additional rules for whether the client can modify accounts it owns. In the case of the System program, it allows users to transfer lamports by recognizing transaction signatures. If it sees the client signed the transaction using the keypair's _private key_, it knows the client authorized the token transfer.

In other words, the entire set of accounts owned by a given program can be regarded as a key-value store where a key is the account address and value is program-specific arbitrary binary data. A program author can decide how to manage the program's whole state as possibly many accounts.

After the runtime executes each of the transaction's instructions, it uses the account metadata to verify that none of the access rules were violated. If a program violates an access rule, the runtime discards all account changes made by all instructions and marks the transaction as failed.

## Smart Contracts

Programs don't always require transaction signatures, as the System program does. Instead, the program may manage _smart contracts_. A smart contract is a set of constraints that once satisfied, signal to a program that a token transfer or account update is permitted. For example, one could use the Budget program to create a smart contract that authorizes a token transfer only after some date. Once evidence that the date has past, the contract progresses, and token transfer completes.
