# Embedding the Move Langauge

## Problem

Solana enables developers to write on-chain programs in general purpose programming languages such as C or Rust, but those programs contain Solana-specific mechanisms. For example, there isn't another chain that asks developers to create a Rust module with a `process_instruction(KeyedAccounts)` function. Whenever practical, Solana should offer application developers more portable options.

Until just recently, no popular blockchain offered a language that could expose the value of Solana's massively parallel [runtime](../validator/runtime.md). Solidity contracts, for example, do not separate references to shared data from contract code, and therefore need to be executed serially to ensure deterministic behavior. In practice we see that the most aggressively optimized EVM-based blockchains all seem to peak out around 1,200 TPS - a small fraction of what Solana can do. The Libra project, on the other hand, designed an on-chain programming language called Move that is more suitable for parallel execution. Like Solana's runtime, Move programs depend on accounts for all shared state.

The biggest design difference between Solana's runtime and Libra's Move VM is how they manage safe invocations between modules. Solana took an operating systems approach and Libra took the domain-specific language approach. In the runtime, a module must trap back into the runtime to ensure the caller's module did not write to data owned by the callee. Likewise, when the callee completes, it must again trap back to the runtime to ensure the callee did not write to data owned by the caller. Move, on the other hand, includes an advanced type system that allows these checks to be run by its bytecode verifier. Because Move bytecode can be verified, the cost of verification is paid just once, at the time the module is loaded on-chain. In the runtime, the cost is paid each time a transaction crosses between modules. The difference is similar in spirit to the difference between a dynamically-typed language like Python versus a statically-typed language like Java. Solana's runtime allows applications to be written in general purpose programming languages, but that comes with the cost of runtime checks when jumping between programs.

This proposal attempts to define a way to embed the Move VM such that:

* cross-module invocations within Move do not require the runtime's

  cross-program runtime checks

* Move programs can leverage functionality in other Solana programs and vice

  versa

* Solana's runtime parallelism is exposed to batches of Move and non-Move

  transactions

## Proposed Solution

### Move VM as a Solana loader

The Move VM shall be embedded as a Solana loader under the identifier `MOVE_PROGRAM_ID`, so that Move modules can be marked as `executable` with the VM as its `owner`. This will allow modules to load module dependencies, as well as allow for parallel execution of Move scripts.

All data accounts owned by Move modules must set their owners to the loader, `MOVE_PROGRAM_ID`. Since Move modules encapsulate their account data in the same way Solana programs encapsulate theirs, the Move module owner should be embedded in the account data. The runtime will grant write access to the Move VM, and Move grants access to the module accounts.

### Interacting with Solana programs

To invoke instructions in non-Move programs, Solana would need to extend the Move VM with a `process_instruction()` system call. It would work the same as `process_instruction()` Rust BPF programs.

