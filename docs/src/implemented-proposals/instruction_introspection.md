---
title: instruction introspection
---

## Problem

Some smart contract programs may want to verify that another Instruction is present in a
given Message since that Instruction could be be performing a verification of certain data,
in a precompiled function. (See secp256k1_instruction for an example).

## Solution

Add a new sysvar Sysvar1nstructions1111111111111111111111111 that a program can reference
and received the Message's instruction data inside, and also the index of the current instruction.

Two helper functions to extract this data can be used:

```
fn load_current_index_checked(instruction_data: &[u8]) -> u16;
fn load_instruction_at_checked(instruction_index: usize, instruction_sysvar_account_info: &AccountInfo) -> Result<Instruction>;
```

The runtime will recognize this special instruction, serialize the Message instruction data
for it and also write the current instruction index and then the bpf program can extract the
necessary information from there.

Note: custom serialization of instructions is used because bincode is about 10x slower
in native code and exceeds current SBF instruction limits.
