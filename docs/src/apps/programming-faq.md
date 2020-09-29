---
title: "Programming FAQ"
---

When writing or interacting with Solana programs, there are common questions or challenges that often come up.  Below are resources to help answer these questions.  If not addressed here, Solana [#developers Discord channel](https://discord.gg/RxeGBH) is a great resource.


## InvalidInstructionData

This program error can occur while trying to deserialize the instruction, check
that the structure passed in matches exactly the instruction.  There may be some
padding between fields.  Try packing and unpacking the instruction type `T` to
determine the exact encoding the program expects

```rust
fn unpack<T>(input: &[u8]) -> &T;
fn pack<T>(input: &T) -> &[u8];
```

## InvalidAccountData

This program error can happen for a lot of reasons, Usually, it's caused by
passing an account to the program that the program is not expecting, either in
the wrong position in the instruction or an account not compatible with the
instruction being executed.

An implementation of a program might also cause this error when performing a
cross-program instruction and forgetting to provide the account for the program
that you are calling.

## MissingRequiredSignature

Some instructions require the account to be a signer; this error is returned if an account expected to be signed is not.

An implementation of a program might also cause this error when performing a cross-program invocation that requires a signed program address, but the passed signer seeds passed to `invoke_signed` don't match the signer seeds used to create the program address (`create_program_address`).

## Using float types

Solana programs support a limited subset of Rust's float operations, though they are highly discouraged due to the overhead involved.  If a program attempts to use a float operation that is not supported, the compiler/linker will report an unresolved symbol error.

## CallDepthExceeded

Programs are constrained to run quickly, and to facilitate this, the program's call stack is limited to max depth.  If this error is encountered, then the program itself or it's dependent crate packages have exceeded the max stack depth.

## CallDepth

Cross-program invocations allow programs to invoke other programs directly but the depth is constrained.

## Failure to compiler due to `rand` incompatibility

Programs are constrained to run deterministically, so random numbers are not available.  Sometimes a program may depend on a crate that depends itself on `rand` even if the program does not use any of the random number functionality.  If a program depends on `rand`, the compilation will fail because there is not `get-random` support for Solana.  To work around this dependency issue, add the following dependency to the program's `Cargo.toml`:

```
getrandom = { version = "0.1.14", features = ["dummy"] }
```

## Stack size

Solana programs compile down to Berkley Packet Filter instructions, which use stack frames instead of a variable stack pointer.  Each stack frame is limited to 4KB.  If a program violates that stack frame size, the compiler will report the overrun as a warning.  The reason a warning is reported rather then an error is because some dependent crates may include functionality that violates the stack frame restrictions even if the program doesn't use that functionality.  If the program violates the stack size at runtime, an `AccessViolation` error will be reported.

## Heap size

Programs have access to a heap either directly in C or via the Rust `alloc` APIs.  To facilitate fast allocations, a simple 32KB bump heap is utilized.  The heap does not support `free` or `realloc` so use it wisely.

## Computational constraints

To prevent a program from abusing computation resources, a cap is enforced during execution.  The following operations incur a cost:
- Executing BPF instructions
- Calling system calls (logging, creating program addresses, ...)
- Cross-program invocations incur a base cost and the cost of the program invoked.