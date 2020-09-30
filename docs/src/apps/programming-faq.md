---
title: "Programming FAQ"
---

When writing or interacting with Solana programs, there are common questions or
challenges that often come up.  Below are resources to help answer these
questions.  If not addressed here, the Solana
[#developers](https://discord.gg/RxeGBH) Discord channel is a great resource.

## CallDepth

Cross-program invocations allow programs to invoke other programs directly but
the depth is constrained currently to 1.

## CallDepthExceeded

Programs are constrained to run quickly, and to facilitate this, the program's
call stack is limited to max depth.  If this error is encountered, then the
program itself or its dependent crate packages have exceeded the max stack
depth.

## Computational constraints

To prevent a program from abusing computation resources, a cap is enforced
during execution.  The following operations incur a cost:
- Executing BPF instructions
- Calling system calls (logging, creating program addresses, ...)
- Cross-program invocations incur a base cost and the cost of the program
  invoked.

## Float Rust types

Programs support a limited subset of Rust's float operations, though they
are highly discouraged due to the overhead involved.  If a program attempts to
use a float operation that is not supported, the runtime will report an
unresolved symbol error. Be sure to include integration tests against a local
cluster to ensure the operation is supported.

## Heap size

Programs have access to a heap either directly in C or via the Rust `alloc`
APIs.  To facilitate fast allocations, a simple 32KB bump heap is utilized.  The
heap does not support `free` or `realloc` so use it wisely.

## InvalidAccountData

This program error can happen for a lot of reasons. Usually, it's caused by
passing an account to the program that the program is not expecting, either in
the wrong position in the instruction or an account not compatible with the
instruction being executed.

An implementation of a program might also cause this error when performing a
cross-program instruction and forgetting to provide the account for the program
that you are calling.

## InvalidInstructionData

This program error can occur while trying to deserialize the instruction, check
that the structure passed in matches exactly the instruction.  There may be some
padding between fields.  If the program implements the Rust `Pack` trait then ry
packing and unpacking the instruction type `T` to determine the exact encoding
the program expects:

https://github.com/solana-labs/solana/blob/master/sdk/src/program_pack.rs


## MissingRequiredSignature

Some instructions require the account to be a signer; this error is returned if
an account expected to be signed is not.

An implementation of a program might also cause this error when performing a
cross-program invocation that requires a signed program address, but the passed
signer seeds passed to `invoke_signed` don't match the signer seeds used to
create the program address (`create_program_address`).

## `rand` dependency causes compilation failure

Programs are constrained to run deterministically, so random numbers are not
available.  Sometimes a program may depend on a crate that depends itself on
`rand` even if the program does not use any of the random number functionality.
If a program depends on `rand`, the compilation will fail because there is no
`get-random` support for Solana.  The error will typically look like this:

```
error: target is not supported, for more information see: https://docs.rs/getrandom/#unsupported-targets
   --> /Users/jack/.cargo/registry/src/github.com-1ecc6299db9ec823/getrandom-0.1.14/src/lib.rs:257:9
    |
257 | /         compile_error!("\
258 | |             target is not supported, for more information see: \
259 | |             https://docs.rs/getrandom/#unsupported-targets\
260 | |         ");
    | |___________^
```

To work around this dependency issue, add the following dependency to the
program's `Cargo.toml`:

```
getrandom = { version = "0.1.14", features = ["dummy"] }
```

## Rust restrictions

There are some Rust limitations since programs run in a resource-constrained,
single-threaded environment, and must be deterministic:

- No access to
  - std::fs
  - std::net
  - std::os
  - std::future
  - std::net
  - std::process
  - std::sync
  - std::task
  - std::thread
  - std::time
- Limited access to:
  - std::os
  - rand or any crates that depend on it
- Bincode is extremely computationally expensive in both cycles and call depth and should be avoided
- String formatting should be avoided since it is also computational expensive
- No support for `println!`, `print!`, the Solana SDK helpers in `src/log.rs`
  should be used instead

## Stack size

Solana programs compile down to Berkley Packet Filter instructions, which use
stack frames instead of a variable stack pointer.  Each stack frame is limited
to 4KB.  If a program violates that stack frame size, the compiler will report
the overrun as a warning.  The reason a warning is reported rather than an error
is because some dependent crates may include functionality that violates the
stack frame restrictions even if the program doesn't use that functionality.  If
the program violates the stack size at runtime, an `AccessViolation` error will
be reported.
