---
title: "FAQ"
---

When writing or interacting with Solana programs, there are common questions or
challenges that often come up. Below are resources to help answer these
questions.

If not addressed here, ask on [StackOverflow](https://stackoverflow.com/questions/tagged/solana) with the `solana` tag or check out the Solana [#developer-support](https://discord.gg/RxeGBH)

## Limitations

Developing programs on the Solana blockchain have some inherent limitation associated with them. Below is a list of common limitation that you may run into.

See [Limitations of developing programs](./limitations.md) for more details

## Memory map

The virtual address memory map used by Solana SBF programs is fixed and laid out
as follows

- Program code starts at 0x100000000
- Stack data starts at 0x200000000
- Heap data starts at 0x300000000
- Program input parameters start at 0x400000000

The above virtual addresses are start addresses but programs are given access to
a subset of the memory map. The program will panic if it attempts to read or
write to a virtual address that it was not granted access to, and an
`AccessViolation` error will be returned that contains the address and size of
the attempted violation.

## Heap size

Programs have access to a runtime heap either directly in C or via the Rust
`alloc` APIs. To facilitate fast allocations, a simple 32KB bump heap is
utilized. The heap does not support `free` or `realloc` so use it wisely.

Internally, programs have access to the 32KB memory region starting at virtual
address 0x300000000 and may implement a custom heap based on the program's
specific needs.

- [Rust program heap usage](developing-rust.md#heap)
- [C program heap usage](developing-c.md#heap)

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

## Stack

SBF uses stack frames instead of a variable stack pointer. Each stack frame is
4KB in size.

If a program violates that stack frame size, the compiler will report the
overrun as a warning.

For example: `Error: Function _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E Stack offset of -30728 exceeded max offset of -4096 by 26632 bytes, please minimize large stack variables`

The message identifies which symbol is exceeding its stack frame but the name
might be mangled if it is a Rust or C++ symbol. To demangle a Rust symbol use
[rustfilt](https://github.com/luser/rustfilt). The above warning came from a
Rust program, so the demangled symbol name is:

```bash
$ rustfilt _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
curve25519_dalek::edwards::EdwardsBasepointTable::create
```

To demangle a C++ symbol use `c++filt` from binutils.

The reason a warning is reported rather than an error is because some dependent
crates may include functionality that violates the stack frame restrictions even
if the program doesn't use that functionality. If the program violates the stack
size at runtime, an `AccessViolation` error will be reported.

SBF stack frames occupy a virtual address range starting at 0x200000000.
