---
title: "Limitations"
---

Developing programs on the Solana blockchain have some inherent limitation associated with them. Below is a list of common limitation that you may run into.

## Rust libraries

Since Rust based on-chain programs must run be deterministic while running in a resource-constrained, single-threaded environment, they have some limitations on various libraries.

See [Developing with Rust - Restrictions](./developing-rust.md#restrictions) for a detailed breakdown these restrictions and limitations.

## Compute budget

To prevent abuse of the blockchain's computational resources, each transaction is allocated a [compute budget](./../../terminology.md#compute-budget). Exceeding this compute budget will result in the transaction failing.

See [computational constraints](../programming-model/runtime.md#compute-budget) in the Runtime for more specific details.

## Call stack depth - `CallDepthExceeded` error

Solana programs are constrained to run quickly, and to facilitate this, the program's call stack is limited to a max depth of **64 frames**.

When a program exceeds the allowed call stack depth limit, it will receive the `CallDepthExceeded` error.

## CPI call depth - `CallDepth` error

Cross-program invocations allow programs to invoke other programs directly, but the depth is constrained currently to `4`.

When a program exceeds the allowed [cross-program invocation call depth](../programming-model/calling-between-programs.md#call-depth), it will receive a `CallDepth` error

## Float Rust types support

Programs support a limited subset of Rust's float operations. If a program
attempts to use a float operation that is not supported, the runtime will report
an unresolved symbol error.

Float operations are performed via software libraries, specifically LLVM's float
built-ins. Due to the software emulated, they consume more compute units than
integer operations. In general, fixed point operations are recommended where
possible.

The Solana Program Library math tests will report the performance of some math
operations: https://github.com/solana-labs/solana-program-library/tree/master/libraries/math

To run the test: sync the repo and run:

```sh
cargo test-sbf -- --nocapture --test-threads=1
```

Recent results show the float operations take more instructions compared to
integers equivalents. Fixed point implementations may vary but will also be
less than the float equivalents:

```
          u64   f32
Multiply    8   176
Divide      9   219
```

## Static writable data

Program shared objects do not support writable shared data. Programs are shared
between multiple parallel executions using the same shared read-only code and
data. This means that developers should not include any static writable or
global variables in programs. In the future a copy-on-write mechanism could be
added to support writable data.

## Signed division

The SBF instruction set does not support
[signed division](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.Html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation). Adding a signed division instruction is a consideration.
