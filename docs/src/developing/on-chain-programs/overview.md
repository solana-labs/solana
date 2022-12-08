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

SBF program deployment is the process of uploading a BPF shared object into a
program account's data and marking the account executable. A client breaks the
SBF shared object into smaller pieces and sends them as the instruction data of
[`Write`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13)
instructions to the loader where loader writes that data into the program's
account data. Once all the pieces are received the client sends a
[`Finalize`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30)
instruction to the loader, the loader then validates that the SBF data is valid
and marks the program account as _executable_. Once the program account is
marked executable, subsequent transactions may issue instructions for that
program to process.

When an instruction is directed at an executable SBF program the loader
configures the program's execution environment, serializes the program's input
parameters, calls the program's entrypoint, and reports any errors encountered.

For further information see [deploying](deploying.md)

### Input Parameter Serialization

SBF loaders serialize the program input parameters into a byte array that is
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
