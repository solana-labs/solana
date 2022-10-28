---
title: "FAQ"
---

When writing or interacting with Solana programs, there are common questions or
challenges that often come up. Below are resources to help answer these
questions.

If not addressed here, ask on [StackOverflow](https://stackoverflow.com/questions/tagged/solana) with the `solana` tag or check out the Solana [#developer-support](https://discord.gg/RxeGBH)

## `CallDepth` error

This error means that that cross-program invocation exceeded the allowed
invocation call depth.

See [cross-program invocation Call
Depth](developing/programming-model/calling-between-programs.md#call-depth)

## `CallDepthExceeded` error

This error means the SBF stack depth was exceeded.

See [call depth](overview.md#call-depth)

## Computational constraints

See [computational
constraints](developing/programming-model/runtime.md#compute-budget)

## Float Rust types

See [float support](overview.md#float-support)

## Heap size

See [heap](overview.md#heap)

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
that the structure passed in matches exactly the instruction. There may be some
padding between fields. If the program implements the Rust `Pack` trait then try
packing and unpacking the instruction type `T` to determine the exact encoding
the program expects:

https://github.com/solana-labs/solana/blob/v1.4/sdk/program/src/program_pack.rs

## MissingRequiredSignature

Some instructions require the account to be a signer; this error is returned if
an account is expected to be signed but is not.

An implementation of a program might also cause this error when performing a
cross-program invocation that requires a signed program address, but the passed
signer seeds passed to [`invoke_signed`](developing/programming-model/calling-between-programs.md)
don't match the signer seeds used to create the program address
[`create_program_address`](developing/programming-model/calling-between-programs.md#program-derived-addresses).

## `rand` Rust dependency causes compilation failure

See [Rust Project Dependencies](developing-rust.md#project-dependencies)

## Rust restrictions

See [Rust restrictions](developing-rust.md#restrictions)

## Stack size

See [stack](overview.md#stack)
