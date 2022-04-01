---
title: "Overview"
---

Developers can write and deploy their own programs to the Solana blockchain.

The [Helloworld example](examples.md#helloworld) is a good starting place to see
how a program is written, built, deployed, and interacted with on-chain.

## Berkeley Packet Filter (BPF)

Solana on-chain programs are compiled via the [LLVM compiler
infrastructure](https://llvm.org/) to an [Executable and Linkable Format
(ELF)](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format) containing
a variation of the [Berkeley Packet Filter
(BPF)](https://en.wikipedia.org/wiki/Berkeley_Packet_Filter) bytecode.

Because Solana uses the LLVM compiler infrastructure, a program may be written
in any programming language that can target the LLVM's BPF backend. Solana
currently supports writing programs in Rust and C/C++.

BPF provides an efficient [instruction
set](https://github.com/iovisor/bpf-docs/blob/master/eBPF.md) that can be
executed in an interpreted virtual machine or as efficient just-in-time compiled
native instructions.

## Memory map

The virtual address memory map used by Solana BPF programs is fixed and laid out
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

## Stack

BPF uses stack frames instead of a variable stack pointer. Each stack frame is
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

BPF stack frames occupy a virtual address range starting at 0x200000000.

## Call Depth

Programs are constrained to run quickly, and to facilitate this, the program's
call stack is limited to a max depth of 64 frames.

## Heap

Programs have access to a runtime heap either directly in C or via the Rust
`alloc` APIs. To facilitate fast allocations, a simple 32KB bump heap is
utilized. The heap does not support `free` or `realloc` so use it wisely.

Internally, programs have access to the 32KB memory region starting at virtual
address 0x300000000 and may implement a custom heap based on the program's
specific needs.

- [Rust program heap usage](developing-rust.md#heap)
- [C program heap usage](developing-c.md#heap)

## Float Support

Programs support a limited subset of Rust's float operations, if a program
attempts to use a float operation that is not supported, the runtime will report
an unresolved symbol error.

Float operations are performed via software libraries, specifically LLVM's float
builtins. Due to the software emulated they consume more compute units than
integer operations. In general, fixed point operations are recommended where
possible.

The Solana Program Library math tests will report the performance of some math
operations:
https://github.com/solana-labs/solana-program-library/tree/master/libraries/math

To run the test, sync the repo, and run:

`$ cargo test-bpf -- --nocapture --test-threads=1`

Recent results show the float operations take more instructions compared to
integers equivalents. Fixed point implementations may vary but will also be
less than the float equivalents:

```
         u64   f32
Multipy    8   176
Divide     9   219
```

## Static Writable Data

Program shared objects do not support writable shared data. Programs are shared
between multiple parallel executions using the same shared read-only code and
data. This means that developers should not include any static writable or
global variables in programs. In the future a copy-on-write mechanism could be
added to support writable data.

## Signed division

The BPF instruction set does not support [signed
division](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation).
Adding a signed division instruction is a consideration.

## Loaders

Programs are deployed with and executed by runtime loaders, currently there are
two supported loaders [BPF
Loader](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)
and [BPF loader
deprecated](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Loaders may support different application binary interfaces so developers must
write their programs for and deploy them to the same loader. If a program
written for one loader is deployed to a different one the result is usually a
`AccessViolation` error due to mismatched deserialization of the program's input
parameters.

For all practical purposes program should always be written to target the latest
BPF loader and the latest loader is the default for the command-line interface
and the javascript APIs.

For language specific information about implementing a program for a particular
loader see:

- [Rust program entrypoints](developing-rust.md#program-entrypoint)
- [C program entrypoints](developing-c.md#program-entrypoint)

### Deployment

BPF program deployment is the process of uploading a BPF shared object into a
program account's data and marking the account executable. A client breaks the
BPF shared object into smaller pieces and sends them as the instruction data of
[`Write`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13)
instructions to the loader where loader writes that data into the program's
account data. Once all the pieces are received the client sends a
[`Finalize`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30)
instruction to the loader, the loader then validates that the BPF data is valid
and marks the program account as _executable_. Once the program account is
marked executable, subsequent transactions may issue instructions for that
program to process.

When an instruction is directed at an executable BPF program the loader
configures the program's execution environment, serializes the program's input
parameters, calls the program's entrypoint, and reports any errors encountered.

For further information see [deploying](deploying.md)

### Input Parameter Serialization

BPF loaders serialize the program input parameters into a byte array that is
then passed to the program's entrypoint, where the program is responsible for
deserializing it on-chain. One of the changes between the deprecated loader and
the current loader is that the input parameters are serialized in a way that
results in various parameters falling on aligned offsets within the aligned byte
array. This allows deserialization implementations to directly reference the
byte array and provide aligned pointers to the program.

For language specific information about serialization see:

- [Rust program parameter
  deserialization](developing-rust.md#parameter-deserialization)
- [C program parameter
  deserialization](developing-c.md#parameter-deserialization)

The latest loader serializes the program input parameters as follows (all
encoding is little endian):

- 8 bytes unsigned number of accounts
- For each account
  - 1 byte indicating if this is a duplicate account, if not a duplicate then
    the value is 0xff, otherwise the value is the index of the account it is a
    duplicate of.
  - If duplicate: 7 bytes of padding
  - If not duplicate:
    - 1 byte boolean, true if account is a signer
    - 1 byte boolean, true if account is writable
    - 1 byte boolean, true if account is executable
    - 4 bytes of padding
    - 32 bytes of the account public key
    - 32 bytes of the account's owner public key
    - 8 bytes unsigned number of lamports owned by the account
    - 8 bytes unsigned number of bytes of account data
    - x bytes of account data
    - 10k bytes of padding, used for realloc
    - enough padding to align the offset to 8 bytes.
    - 8 bytes rent epoch
- 8 bytes of unsigned number of instruction data
- x bytes of instruction data
- 32 bytes of the program id
