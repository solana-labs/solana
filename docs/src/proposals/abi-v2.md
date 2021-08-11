---
title: ABI v2
---

ABI between loader and program entrypoint, as well as syscalls such as (cross program) invocation and account reallocation.

## Motivation
The reason we need a new serialization format and thus also a new ABI is because the existing one is not extensibile and stores the account data in-place / in-stream. This prevents accounts from being memory mapped individually and also hinders reallocation of accounts (changing their size). Furthermore, there is a lot of potential for optimizations such as encoding the meta data only once per transaction and partially per instruction.

## Existing Encoding Standards
Information about each entry (attribute value pair) which is either encoded explicitly or can be implicitly gained from the context:
- Attribute (semantics) as a tag or name string
- Data type / encoding
- Value length
- Value content

Some [binary data-serialization formats](https://en.wikipedia.org/wiki/Comparison_of_data-serialization_formats#Comparison_of_binary_formats) are [CBOR](https://cbor.io), [Ion](https://amzn.github.io/ion-docs/), [MsgPack](https://msgpack.org), [Protobuf](https://github.com/protocolbuffers/protobuf).
We currently only have CBOR in our Cargo.lock, so everything else would add a dependency.
However, non of these support indirections / references / offsets / pointers and they are all in row-major order (see next point). Therefor, we should design and implement our own extensibile serialization format.

## Nesting order
Borrowing the terms from table based data base systems:
- Row-major encoding (inner loop over attributes, outer loop over accounts) means first the values of all attributes of one account are encoded, then comes the next account.
- Column-major encoding (inner loop over accounts, outer loop over attributes) means first the values of one attribute for all accounts are encoded, then comes the next attribute.

Column-major encoding has the following advantages in our use case:
- Extensibility on an attribute level (but not per account level as that would be wasteful).
- Read / write access control per attribute (not per account meta data, but the account data is handled separately).
- Splitting meta data into a local instruction context and a global transaction context.

## Design
This section describes how the new encoding works in general and how we would use it for the new ABI specifically.

### Encoding
Everything is in little endian.

Associative map:
- `u16`: number_of_entries (attribute value pairs)
- `[u16; number_of_entries]`: Attributes sorted ascending (for binary search)
- `[u8; number_of_entries]`: Branching of values: Direct nesting, indirect nesting, direct leaf, indirect leaf
- `[u32; number_of_entries]`: Offsets into values (cummulative value lengths)
- Values

### Memory Mapping / Regions
Additional to the mandatory RBPF memory regions (null, program, stack, heap) we would add `2 + number_of_accounts_in_instruction` memory regions to the VM mapping.

#### Read-only meta data region:
- Transaction context:
  - `number_of_accounts`: Natural number
  - `account_key`: Vector of pubkeys for all accounts
  - `account_is_executable`: Bit vector for all accounts
  - `account_owner`: Vector of pubkeys for all accounts
  - `account_lamports`: Vector of natural numbers for all accounts
  - `account_data_length`: Vector of natural numbers for all accounts
  - `account_data`: Vector of pointers for all accounts
  - `account_rent_epoch` (deprecated): Vector of natural numbers for all accounts
- Instruction context:
  - `invoke_stack_frame`: Offset into nested structure
  - Invoke stack frames:
    - `program_account_index`: Natural number index into the transaction accounts
    - `instruction_data`: Blob
    - `number_of_accounts`: Natural number
    - `account_indices`: Vector of natural number indices into the transaction accounts
    - `account_is_signer`: Bit vector for all accounts referenced by account_indices
    - `account_is_writable`: Bit vector for all accounts referenced by account_indices

#### Read-write meta data region:
- Instruction context:
    - `account_owner`: Vector of pubkeys for writable accounts
    - `account_lamports`: Vector of natural numbers for writable accounts

#### Individual account data regions:
- One for each account in the instruction
- Read-only or read-write depends on instruction context
- `data`: Blob

### Usage / Integration
Replace `KeyedAccounts`, `serialize_parameters`, `deserialize_parameters` and parts of the `InvokeContext` with a new interface which directly operates on the new encoding. It would be used by the runtime and programs alike.

As we have already seen in [#15410](https://github.com/solana-labs/solana/pull/15410) this will most likely cause problems with the Rust borrow checker, because we can not have multiple mutable references to the `InvokeContext`. So the workaround using indices as account handles might continue to be necessary.