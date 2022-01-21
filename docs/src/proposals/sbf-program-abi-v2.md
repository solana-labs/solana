---
title: SBF Program ABI v2
---

ABI between loader and program entrypoint, as well as syscalls such as (cross program) invocation and account reallocation.

## Motivation
The reason we need a new serialization format and thus also a new ABI is because the existing one is not extensibile and stores the account data in-place / in-stream. This prevents accounts from being memory mapped individually and also hinders reallocation of accounts (changing their size).

### Design Goals
- Use memory mapping to share account data with the program VMs directly to reduce the worst case of six copies of each account per instruction to zero copies
- In general, expose all static information to the programs by sharing it through memory mapping instead of providing syscalls to request it
- Stop invalid access patterns (e.g. writing to read only accounts) immediately, instead of relying on PreAccount to validate everything after each instruction
- Extensibility: Allow changes in the interfaces attributes
- Reallocation / resizing: Allow changes in the account sizes
- Creation: Allow changes in the number of accounts in the transaction
- Have an unified interface for SBF programs, built-in programs, tests, mockups and the runtime

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
- Row-major encoding or Array-of-Structures, (inner loop over attributes, outer loop over accounts) means first the values of all attributes of one account are encoded, then comes the next account.
- Column-major encoding or Structure-of-Arrays, (inner loop over accounts, outer loop over attributes) means first the values of one attribute for all accounts are encoded, then comes the next attribute.

Advantages of row-major encoding:
- Support creating accounts in the transaction

Advantages column-major encoding:
- Extensibility of attributes
- Read / write access control per attribute (not per account meta data, but the account data is handled separately).
- Splitting meta data into a local instruction context and a global transaction context.

## Design
This section describes how the new encoding works in general and how we would use it for the new ABI specifically.

### Encoding
Everything is in little endian and padded to alignment for fast access.
Tight packing makes no sense as the encoded structure does not need to be copied in and out of the VMs, making the additional memory footprint of padding in the meta data irrelevant.

### Memory Mapping / Regions
Additional to the mandatory RBPF memory regions (null, program, stack, heap) we would add the following memory regions to the VM mapping:
- Readonly attributes:
  - Account Pubkeys
  - Owner Pubkeys
  - Is-executable flags
  - Rent epochs
  - Lamports of readonly accounts (as defined by the current instruction)
  - All entries of the instruction trace except for the last
- Writable attributes:
  - Lamports of writable accounts (as defined by the current instruction)
  - Only the last (reserved empty) slot on the instruction trace
- Account data regions:
  - One mapped memory region for each account in the instruction
  - Read-only or read-write permission are defined by the current instruction

#### Writable Attributes
Alternative options:
- Three regions per account: Read-only metadata, writable metadata, account data
- All meta data in readonly region, plus duplicate for writable metadata in extra region
- Two regions: Definitely read-only metadata and potentially writable metadata including readonly accounts and unmapped accounts

#### Host and Guest (VM) Address Space for Indirections / Pointers
Host pointers can not be exposed to the guest at all, not even in an encrypted manner. That is because every validator can allocate the accounts somewhere else, so their pointers would diverge and it would be visible to the user programs.

Guest pointers are stable across the entire transaction too, meaning that the address space of the accounts stays reserved in instructions which do not have these accounts mapped in the VM.

### Interface Changes
The following Pubkey based structures will be changed:
- PreAccount: Can be dropped without a replacement
- Message: Partially exposed in TransactionContext
- Instruction and AccountMeta: Replaced by InstructionContext
- KeyedAccount and AccountInfo: Replaced by BorrowedAccount

Built-in programs and all tests and mock-ups will be refactored to use these new structures directly. SBF programs will be required to be redeployed with a new loader in order to use this new interface. Additionally we will provide helper functions to bridge the gap of these interfaces at the users cost, meaning that they can go cheaper if they adapt to using the new interface directly.

#### TransactionContext
- account_keys: `Pin<Box<[Pubkey]>>`
- accounts: `Pin<Box<[RefCell<AccountSharedData>]>>`
- instruction_trace: `Vec<InstructionContext>`
- return_data: `(Pubkey, Vec<u8>)`

Instead of having a separate invocation stack and popping these frames and copy them into the instruction recorder, ABIv2 can simply operate on a sequence annotated with their invocation depth: The instruction trace.

In ABIv1 return data was implemented by a setter and a getter syscall for copying data forth and back between userspace and runtime. In ABIv2, we can use shared memory in the transaction context instead. So, the two syscalls will be obsolete as there is no need to trigger anything in the runtime.

#### InstructionContext
- invocation_depth: `usize`
- instruction_data: `Vec<u8>`
- program_accounts: `Vec<usize>`
- index_in_transaction: `Vec<usize>`
- index_in_caller: `Vec<usize>`
- is_signer: `Vec<bool>`
- is_writable: `Vec<bool>`

One difference to the Instruction structure is, that while the user could encode duplicate account entries with different permissions, ABIv1 does unify the permissions of all aliasing accounts. This will not be the case in ABIv2.

#### BorrowedAccount
- transaction_context: `&TransactionContext`
- instruction_context: `&InstructionContext`
- index_in_transaction: `usize`
- index_in_instruction: `usize`
- account: `RefMut<AccountSharedData>`

### Syscalls
There are some interactions which will still require syscalls to notify the runtime that the program requested changes.

#### Cross Program Invocation (CPI)
The runtime automatically allocates an empty InstructionContext (mapped as writable) behind the currently active one. If a program wants to call another one it simply writes directly into that InstructionContext and configures it. Then the program only needs a syscall to trigger the validation and execution by the runtime.

#### Account Reallocation / Resizing
TODO