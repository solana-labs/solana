---
title: SBF Program ABI v2
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
However, none of these support indirections / references / offsets / pointers and they are all in row-major order (see next point). Therefore, we should design and implement our own extensibile serialization format.

## Nesting order
Borrowing the terms from table based database systems:
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

```Rust
struct Map {
  number_of_entries: u16,
  attribute: [u16; number_of_entries],
  value_offset: [u32; number_of_entries],
  values: [Value],
}

struct FatPtr {
  address: u64,
  length: u32,
}

union Value {
  Direct([u8]),
  Indirect([FatPtr]),
}
```

### Memory Mapping / Regions
Additional to the mandatory RBPF memory regions (null, program, stack, heap) we would add `2 + number_of_accounts_in_instruction` memory regions to the VM mapping.

The idea is to serialize the meta data of all accounts once per transaction, and only serialize a few attributes like `AccountIsSigner` and `AccountIsWritable` which are currently stored in `KeyedAccounts` per instruction. Then these two meta data regions and the `number_of_accounts_in_instruction` account data regions are mapped directly by pointers and not copied anymore. Furthermore, the `PreAccount`s become obsolete as well because the writability of an account can be enforced by the memory mapping.

#### Read-only meta data region:
- Transaction context:
  - `NumberOfAccountsInTransaction = 0`: `u32`
  - `AccountKey = 1`: `[Pubkey; NumberOfAccountsInTransaction]`
  - `AccountIsExecutable = 2`: `[bool; NumberOfAccountsInTransaction]`
  - `AccountOwner = 3`: `[Pubkey; NumberOfAccountsInTransaction]`
  - `AccountLamports = 4`: `[u64; NumberOfAccountsInTransaction]`
  - `AccountData = 5`: `[&[u8]; NumberOfAccountsInTransaction]`
  - `InvocationStackHeight = 6`: `u16`
  - `InvocationStack = 7`: `[Map; InvocationStackHeight]`
- Instruction context:
  - `InstructionData = 8`: `[u8]`
  - `NumberOfAccountsInInstruction = 9`: `u16`
  - `InstructionAccountIndices = 10`: `[u32; NumberOfAccountsInInstruction]`
  - `ProgramAccountIndex = 11`: `u32`
  - `AccountIsSigner = 12`: `[bool; NumberOfAccountsInInstruction]`
  - `AccountIsWritable = 13`: `[bool; NumberOfAccountsInInstruction]`
  - `WritableAttributes = 14`: `Map`

#### Read-write meta data region:
- Instruction context:
  - `AccountIsExecutable`,
  - `AccountOwner`,
  - `AccountLamports`,

#### Individual account data regions:
- One mapped memory region for each account in the instruction
- But virtual and physical addresses stay the same throughout the transaction
- Read-only or read-write depends on the instruction context
- Content: `[u8]`

### Syscalls

#### Cross Program Invocation
TODO

#### Account Reallocation / Resizing
TODO

### Usage / Integration
Replace `KeyedAccounts`, `serialize_parameters`, `deserialize_parameters` and parts of the `InvokeContext` with a new interface which directly operates on the new encoding. It would be used by the runtime and programs alike.

As we have already seen in [#15410](https://github.com/solana-labs/solana/pull/15410) this will most likely cause problems with the Rust borrow checker, because we can not have multiple mutable references to the `InvokeContext`. So the workaround using indices as account handles might continue to be necessary.