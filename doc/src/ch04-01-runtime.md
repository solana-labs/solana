# The LAMPORT execution environment

## Introduction

With LAMPORT (Language-Agnostic, Memory-oriented, Parallel-friendly, Optimized
Run-Time), we can execute smart contracts concurrently, and written in the
client’s choice of programming language. Furthermore, we demonstrate Solana’s
built-in smart contract language Budget can target LAMPORT without any loss in
performance. The two features that allow LAMPORT to work:

Client-owned memory identified by public keys. By declaring ownership upfront
and separating the program’s state from the program, the runtime knows which
contracts can safely be executed concurrently.  Solana’s blockchain-encoded VDF
tells validator nodes at precisely what times they need to end up in the same
state. Between those times, they are free to introduce non-deterministic
behavior as-needed to improve execution times.

## Toolchain Stack

<img alt="SDK tools" src="img/sdk-tools.svg" class="center"/>

As shown in the diagram above an untrusted client, creates a program in the
front-end language of her choice, (like C/C++/Rust/Lua), and compiles it with
LLVM to a position independent shared object ELF, targeting BPF bytecode.
Solana will safely load and execute the ELF.

## Runtime

The goal with the runtime is to have a general purpose execution environment
that is highly parallelizable.  To achieve this goal the runtime forces each
Instruction to specify all of its memory dependencies up front, and therefore a
single Instruction cannot cause a dynamic memory allocation.  An explicit
Instruction for memory allocation from the `SystemProgram::CreateAccount` is
the only way to allocate new memory in the engine.  A Transaction may compose
multiple Instruction, including `SystemProgram::CreateAccount`, into a single
atomic sequence which allows for memory allocation to achieve a result that is
similar to dynamic allocation.


### State

State is addressed by an Account which is at the moment simply the Pubkey.  Our
goal is to eliminate memory allocation from within the program itself.  Thus
the client of the program provides all the state that is necessary for the
program to execute in the transaction itself.  The runtime interacts with the
program through an entry point with a well defined interface.  The userdata
stored in an Account is an opaque type to the runtime, a `Vec<u8>`, the
contents of which the program code has full control over.

The Transaction structure specifies a list of Pubkey's and signatures for those
keys and a sequential list of instructions that will operate over the state's
associated with the `account_keys`.  For the transaction to be committed all
the instructions must execute successfully, if any abort the whole transaction
fails to commit.

### Account structure Accounts maintain token state as well as program specific
memory.

# Transaction Engine

At its core, the engine looks up all the Pubkeys maps them to accounts and
routs them to the `program_id` entry point.

## Execution

Transactions are batched and processed in a pipeline

<img alt="LAMPORT pipeline" src="img/lamport.svg" class="center"/>

At the `execute` stage, the loaded pages have no data dependencies, so all the
programs can be executed in parallel. 

The runtime enforces the following rules:

1. The `program_id` code is the only code that will modify the contents of
   `Account::userdata` of Account's that have been assigned to it.  This means
that upon assignment userdata vector is guaranteed to be `0`.
2. Total balances on all the accounts is equal before and after execution of a
   Transaction.
3. Balances of each of the accounts not assigned to `program_id` must be equal
   to or greater after the Transaction than before the transaction.
4. All Instructions in the Transaction executed without a failure.

## Entry Point Execution of the program involves mapping the Program's public
key to an entry point which takes a pointer to the transaction, and an array of
loaded pages.

## System Interface

The interface is best described by the `Instruction::userdata` that the
user encodes. 
* `CreateAccount` - This allows the user to create and assign an Account to a
  Program.
* `Assign` - allows the user to assign an existing account to a `Program`. 
* `Move`  - moves tokens between `Account`s that are associated with
  `SystemProgram`.  This cannot be used to move tokens of other `Account`s.
Programs need to implement their own version of Move.

## Notes

1. There is no dynamic memory allocation.  Client's need to call the
`SystemProgram` to create memory before passing it to another program.  This
Instruction can be composed into a single Transaction with the call to the
program itself.
2. Runtime guarantees that when memory is assigned to the `Program` it is zero
initialized.
3. Runtime guarantees that `Program`'s code is the only thing that can modify
memory that its assigned to
4. Runtime guarantees that the `Program` can only spend tokens that are in
`Account`s that are assigned to it
5. Runtime guarantees the balances belonging to `Account`s are balanced before
and after the transaction
6. Runtime guarantees that multiple instructions all executed successfully when
a transaction is committed.

# Future Work

* [Continuations and Signals for long running
  Transactions](https://github.com/solana-labs/solana/issues/1485)

