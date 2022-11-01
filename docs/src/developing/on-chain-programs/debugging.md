---
title: "Debugging Programs"
---

Solana programs run on-chain, so debugging them in the wild can be challenging.
To make debugging programs easier, developers can write unit tests that directly
test their program's execution via the Solana runtime, or run a local cluster
that will allow RPC clients to interact with their program.

## Running unit tests

- [Testing with Rust](developing-rust.md#how-to-test)
- [Testing with C](developing-c.md#how-to-test)

## Logging

During program execution both the runtime and the program log status and error
messages.

For information about how to log from a program see the language specific
documentation:

- [Logging from a Rust program](developing-rust.md#logging)
- [Logging from a C program](developing-c.md#logging)

When running a local cluster the logs are written to stdout as long as they are
enabled via the `RUST_LOG` log mask. From the perspective of program
development it is helpful to focus on just the runtime and program logs and not
the rest of the cluster logs. To focus in on program specific information the
following log mask is recommended:

`export RUST_LOG=solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=info,solana_bpf_loader=debug,solana_rbpf=debug`

Log messages coming directly from the program (not the runtime) will be
displayed in the form:

`Program log: <user defined message>`

## Error Handling

The amount of information that can be communicated via a transaction error is
limited but there are many points of possible failures. The following are
possible failure points and information about what errors to expect and where to
get more information:

- The SBF loader may fail to parse the program, this should not happen since the
  loader has already _finalized_ the program's account data.
  - `InstructionError::InvalidAccountData` will be returned as part of the
    transaction error.
- The SBF loader may fail to setup the program's execution environment
  - `InstructionError::Custom(0x0b9f_0001)` will be returned as part of the
    transaction error. "0x0b9f_0001" is the hexadecimal representation of
    [`VirtualMachineCreationFailed`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/programs/bpf_loader/src/lib.rs#L44).
- The SBF loader may have detected a fatal error during program executions
  (things like panics, memory violations, system call errors, etc...)
  - `InstructionError::Custom(0x0b9f_0002)` will be returned as part of the
    transaction error. "0x0b9f_0002" is the hexadecimal representation of
    [`VirtualMachineFailedToRunProgram`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/programs/bpf_loader/src/lib.rs#L46).
- The program itself may return an error
  - `InstructionError::Custom(<user defined value>)` will be returned. The
    "user defined value" must not conflict with any of the [builtin runtime
    program
    errors](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/program_error.rs#L87).
    Programs typically use enumeration types to define error codes starting at
    zero so they won't conflict.

In the case of `VirtualMachineFailedToRunProgram` errors, more information about
the specifics of what failed are written to the [program's execution
logs](debugging.md#logging).

For example, an access violation involving the stack will look something like
this:

`SBF program 4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM failed: out of bounds memory store (insn #615), addr 0x200001e38/8`

## Monitoring Compute Budget Consumption

The program can log the remaining number of compute units it will be allowed
before program execution is halted. Programs can use these logs to wrap
operations they wish to profile.

- [Log the remaining compute units from a Rust
  program](developing-rust.md#compute-budget)
- [Log the remaining compute units from a C
  program](developing-c.md#compute-budget)

See [compute budget](developing/programming-model/runtime.md#compute-budget)
for more information.

## ELF Dump

The SBF shared object internals can be dumped to a text file to gain more
insight into a program's composition and what it may be doing at runtime.

- [Create a dump file of a Rust program](developing-rust.md#elf-dump)
- [Create a dump file of a C program](developing-c.md#elf-dump)

## Instruction Tracing

During execution the runtime SBF interpreter can be configured to log a trace
message for each SBF instruction executed. This can be very helpful for things
like pin-pointing the runtime context leading up to a memory access violation.

The trace logs together with the [ELF dump](#elf-dump) can provide a lot of
insight (though the traces produce a lot of information).

To turn on SBF interpreter trace messages in a local cluster configure the
`solana_rbpf` level in `RUST_LOG` to `trace`. For example:

`export RUST_LOG=solana_rbpf=trace`
