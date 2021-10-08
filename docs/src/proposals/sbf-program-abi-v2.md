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
Additional to the mandatory RBPF memory regions (null, program, stack, heap) we would add `3 + NumberOfAccountsInTransaction + InvocationDepthMax` memory regions to the VM mapping.

The idea is to serialize the meta data of all accounts once per transaction, and only serialize a few attributes like `AccountIsSigner` and `AccountIsWritable` which are currently stored in `KeyedAccounts` per instruction. Then these meta data and account data regions are mapped directly by pointers and not copied anymore. Furthermore, the `PreAccount`s become obsolete as well because the write protection of an account can be enforced by the memory mapping.

#### Regions and their Attributes:
- TransactionContext metadata regions:
  - Read-only and writable attributes are separated into two metadata regions
  - `NumberOfAccountsInTransaction = 0`: `u32`
  - `AccountKey = 1`: `[Pubkey; NumberOfAccountsInTransaction]`
  - `AccountIsExecutable = 2`: `[bool; NumberOfAccountsInTransaction]` (Writable)
  - `AccountOwner = 3`: `[Pubkey; NumberOfAccountsInTransaction]` (Writable)
  - `AccountLamports = 4`: `[u64; NumberOfAccountsInTransaction]` (Writable)
  - `AccountData = 5`: `[AccountData; NumberOfAccountsInTransaction]`
  - `ReturnData = 6`: `[u8; program::MAX_RETURN_DATA]` (Writable)
  - `InvocationDepth = 7`: `u16`
  - `InvocationDepthMax = 8`: `u16`
  - `InvocationStack = 9`: `[InstructionContext; InvocationDepthMax]`
- InstructionContext metadata regions:
  - Read-only unless last invocation frame (for CPI)
  - `InstructionData = 10`: `[u8]`
  - `NumberOfAccountsInInstruction = 11`: `u16`
  - `NumberOfProgramsInInstruction = 12`: `u16`
  - `InstructionAccountIndices = 13`: `[u32; NumberOfAccountsInInstruction]`
  - `AccountIsSigner = 14`: `[bool; NumberOfAccountsInInstruction]`
  - `AccountIsWritable = 15`: `[bool; NumberOfAccountsInInstruction]`
- AccountData regions:
  - One mapped memory region for each account in the instruction
  - But virtual and physical addresses stay the same throughout the transaction (useful for CPI)
  - Read-only or read-write depends on the instruction context
  - Content: `[u8]`

#### Writable Attributes
Alternative options:
- Three regions per account: Read-only metadata, writable metadata, account data
- All meta data in readonly region, plus duplicate for writable metadata in extra region
- Two regions: Definitely read-only metadata and potentially writable metadata including readonly accounts and unmapped accounts

#### Encoding Alignment
Alternative options:
- Tightly pack everything
- Tightly pack attributes, pad values
- Pad attributes and their values

#### Host and Guest (VM) Address Space for Indirections / Pointers
Alternative options:
- Store guest pointers only, map host on demand
- Translate on every context switch (invocation, exit, syscall)
- Store both guest and host, but encrypt host pointers

### Interface

#### Instruction Structs
TODO: Design a structured interface to deal with the account indices of different instructions.
Replacing: `keyed_account_at_index(keyed_accounts, first_instruction_account + x)`

#### Tests & Mockups
TODO: Design a better way to create the mocked instruction accounts.
Replacing: `create_keyed_accounts_unified(&keyed_account_tuples)`

### Syscalls

#### Cross Program Invocation (CPI)
The runtime automatically allocates an empty invocation frame (mapped as writable) above the currently active one. If a program wants to call another one it simply writes directly into that empty invocation frame and configures it. Then the program only needs a syscall to trigger the execution and that syscall does not need to copy data forth and back between userspace and runtime anymore. It only has to validate the invocation frame, record the instruction and run the called program.

#### Return Data Accessors
In the old ABI return data was implemented by a setter and a getter syscall for copying data forth and back between userspace and runtime. But now, we can use shared memory in the transaction context instead. So, the two syscalls will be deprecated as there is no need to trigger anything in the runtime.

#### Account Reallocation / Resizing
TODO
