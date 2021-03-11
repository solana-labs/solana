---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run
validator nodes. Unlike third-party programs, the native programs are part of
the validator implementation and can be upgraded as part of cluster upgrades.
Upgrades may occur to add features, fix bugs, or improve performance. Interface
changes to individual instructions should rarely, if ever, occur. Instead, when
change is needed, new instructions are added and previous ones are marked
deprecated. Apps can upgrade on their own timeline without concern of breakages
across upgrades.

For each native program the program id and description each supported
instruction is provided. A transaction can mix and match instructions from different
programs, as well include instructions from on-chain programs.

## System Program

Create accounts and transfer lamports between them

- Program id: `11111111111111111111111111111111`
- Instructions: [SystemInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## Config Program

Add configuration data to the chain and the list of public keys that are permitted to modify it

- Program id: `Config1111111111111111111111111111111111111`
- Instructions: [config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

Unlike the other programs, the Config program does not define any individual
instructions. It has just one implicit instruction, a "store" instruction. Its
instruction data is a set of keys that gate access to the account, and the
data to store in it.

## Stake Program

Create stake accounts and delegate it to validators

- Program id: `Stake11111111111111111111111111111111111111`
- Instructions: [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## Vote Program

Create vote accounts and vote on blocks

- Program id: `Vote111111111111111111111111111111111111111`
- Instructions: [VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## BPF Loader

Add programs to the chain and execute them.

- Program id: `BPFLoader1111111111111111111111111111111111`
- Instructions: [LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_instruction/enum.LoaderInstruction.html)

The BPF Loader marks itself as its "owner" of the executable account it
creates to store your program. When a user invokes an instruction via a
program id, the Solana runtime will load both your executable account and its
owner, the BPF Loader. The runtime then passes your program to the BPF Loader
to process the instruction.

## Secp256k1 Program

Verify secp256k1 public key recovery operations (ecrecover).

- Program id: `KeccakSecp256k11111111111111111111111111111`
- Instructions: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/c1f3f9d27b5f9534f9a37704bae1d690d4335b6b/programs/secp256k1/src/lib.rs#L18)

The secp256k1 program processes an instruction which takes in as the first byte
a count of the following struct serialized in the instruction data:

```
struct Secp256k1SignatureOffsets {
    secp_signature_key_offset: u16,        // offset to [signature,recovery_id,etherum_address] of 64+1+20 bytes
    secp_signature_instruction_index: u8,  // instruction index to find data
    secp_pubkey_offset: u16,               // offset to [signature,recovery_id] of 64+1 bytes
    secp_signature_instruction_index: u8,  // instruction index to find data
    secp_message_data_offset: u16,         // offset to start of message data
    secp_message_data_size: u16,           // size of message data
    secp_message_instruction_index: u8,    // index of instruction data to get message data
}
```

Pseudo code of the operation:
```
process_instruction() {
  for i in 0..count {
      // i'th index values referenced:
      instructions = &transaction.message().instructions
      signature = instructions[secp_signature_instruction_index].data[secp_signature_offset..secp_signature_offset + 64]
      recovery_id = instructions[secp_signature_instruction_index].data[secp_signature_offset + 64]
      ref_eth_pubkey = instructions[secp_pubkey_instruction_index].data[secp_pubkey_offset..secp_pubkey_offset + 32]
      message_hash = keccak256(instructions[secp_message_instruction_index].data[secp_message_data_offset..secp_message_data_offset + secp_message_data_size])
      pubkey = ecrecover(signature, recovery_id, message_hash)
      eth_pubkey = keccak256(pubkey[1..])[12..]
      if eth_pubkey != ref_eth_pubkey {
          return Error
      }
  }
  return Success
}
```

This allows the user to specify any instruction data in the transaction for
signature and message data. By specifying a special instructions sysvar, one can
also receive data from the transaction itself.

Cost of the transaction will count the number of signatures to verify multiplied
by the signature cost verify multiplier.

### Optimization notes

The operation will have to take place after (at least partial) deserialization,
but all inputs come from the transaction data itself, this allows it to be
relatively easy to execute in parallel to transaction processing and PoH
verification.
