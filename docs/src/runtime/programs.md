---
title: "Native Programs in the Solana Runtime"
pagination_label: Runtime Native Programs
sidebar_label: Native Programs
---

Solana contains a small handful of native programs, which are required to run
validator nodes. Unlike third-party programs, the native programs are part of
the validator implementation and can be upgraded as part of cluster upgrades.
Upgrades may occur to add features, fix bugs, or improve performance. Interface
changes to individual instructions should rarely, if ever, occur. Instead, when
change is needed, new instructions are added and previous ones are marked
deprecated. Apps can upgrade on their own timeline without concern of breakages
across upgrades.

For each native program, the program id and the description of each supported
instruction are provided. A transaction can mix and match instructions from different
programs, as well as include instructions from on-chain programs.

## System Program

Create new accounts, allocate account data, assign accounts to owning programs,
transfer lamports from System Program owned accounts and pay transaction fees.

- Program id: `11111111111111111111111111111111`
- Instructions: [SystemInstruction](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/system_instruction/enum.SystemInstruction.html)

## Config Program

Add configuration data to the chain, followed by the list of public keys that
are allowed to modify it

- Program id: `Config1111111111111111111111111111111111111`
- Instructions: [config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

Unlike the other programs, the Config program does not define any individual
instructions. It has just one implicit instruction: "store". Its
instruction data is a set of keys that gate access to the account and the
data to store inside of it.

## Stake Program

Create and manage accounts representing stake and rewards for delegations to
validators.

- Program id: `Stake11111111111111111111111111111111111111`
- Instructions: [StakeInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/stake/instruction/enum.StakeInstruction.html)

## Vote Program

Create and manage accounts that track validator voting state and rewards.

- Program id: `Vote111111111111111111111111111111111111111`
- Instructions: [VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## Address Lookup Table Program

- Program id: `AddressLookupTab1e1111111111111111111111111`
- Instructions: [AddressLookupTableInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/address_lookup_table/instruction/enum.ProgramInstruction.html)

## BPF Loader

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- Instructions: [LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as the "owner" of the executable and
program-data accounts it creates to store your program. When a user invokes an
instruction via a program id, the Solana runtime loads both your program
and its owner, the BPF Upgradeable Loader. The runtime then passes your program
to the BPF Upgradeable Loader for it to process the instruction.

[More information about deployment](../cli/examples/deploy-a-program.md)

## Ed25519 Program

The program for verifying ed25519 signatures. It takes an ed25519 signature, a public key, and a message.
Multiple signatures can be verified. If any of the signatures fail to verify, an error is returned.

- Program id: `Ed25519SigVerify111111111111111111111111111`
- Instructions: [ed25519_instruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/ed25519_instruction/index.html)

The ed25519 program processes an instruction. The first `u8` is a count of the number of
signatures to check, which is followed by a single byte padding. After that, the
following struct is serialized, one for each signature to check.

```
struct Ed25519SignatureOffsets {
    signature_offset: u16,             // offset to ed25519 signature of 64 bytes
    signature_instruction_index: u16,  // instruction index to find signature
    public_key_offset: u16,            // offset to public key of 32 bytes
    public_key_instruction_index: u16, // instruction index to find public key
    message_data_offset: u16,          // offset to start of message data
    message_data_size: u16,            // size of message data
    message_instruction_index: u16,    // index of instruction data to get message data
}
```

The pseudo code of the signature verification:

```
process_instruction() {
    for i in 0..count {
        // i'th index values referenced:
        instructions = &transaction.message().instructions
        instruction_index = ed25519_signature_instruction_index != u16::MAX ? ed25519_signature_instruction_index : current_instruction;
        signature = instructions[instruction_index].data[ed25519_signature_offset..ed25519_signature_offset + 64]
        instruction_index = ed25519_pubkey_instruction_index != u16::MAX ? ed25519_pubkey_instruction_index : current_instruction;
        pubkey = instructions[instruction_index].data[ed25519_pubkey_offset..ed25519_pubkey_offset + 32]
        instruction_index = ed25519_message_instruction_index != u16::MAX ? ed25519_message_instruction_index : current_instruction;
        message = instructions[instruction_index].data[ed25519_message_data_offset..ed25519_message_data_offset + ed25519_message_data_size]
        if pubkey.verify(signature, message) != Success {
            return Error
        }
    }
    return Success
}
```

## Secp256k1 Program

Verify secp256k1 public key recovery operations (ecrecover).

- Program id: `KeccakSecp256k11111111111111111111111111111`
- Instructions: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

The secp256k1 program processes an instruction which takes in as the first byte
a count of the following struct serialized in the instruction data:

```
struct Secp256k1SignatureOffsets {
    secp_signature_offset: u16,            // offset to [signature,recovery_id] of 64+1 bytes
    secp_signature_instruction_index: u8,  // instruction index to find signature
    secp_pubkey_offset: u16,               // offset to ethereum_address pubkey of 20 bytes
    secp_pubkey_instruction_index: u8,     // instruction index to find pubkey
    secp_message_data_offset: u16,         // offset to start of message data
    secp_message_data_size: u16,           // size of message data
    secp_message_instruction_index: u8,    // instruction index to find message data
}
```

The pseudo code of the recovery verification:

```
process_instruction() {
  for i in 0..count {
      // i'th index values referenced:
      instructions = &transaction.message().instructions
      signature = instructions[secp_signature_instruction_index].data[secp_signature_offset..secp_signature_offset + 64]
      recovery_id = instructions[secp_signature_instruction_index].data[secp_signature_offset + 64]
      ref_eth_pubkey = instructions[secp_pubkey_instruction_index].data[secp_pubkey_offset..secp_pubkey_offset + 20]
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

## Secp256r1 Program

The program for verifying secp256r1 signatures. It takes a secp256r1 signature,
a public key, and a message. Up to 8 signatures can be verified. If any of the
signatures fail to verify, an error is returned.

- Program id: `Secp256r1SigVerify1111111111111111111111111`
- Instructions: [secp256r1_instruction](https://docs.rs/solana-secp256r1)

The secp256r1 program processes an instruction. The first `u8` is a count of the number of signatures to check, followed by a single byte padding. After that, the following struct is serialized, one for each signature to check:

```rust
struct Secp256r1SignatureOffsets {
    signature_offset: u16,             // offset to compact secp256r1 signature of 64 bytes
    signature_instruction_index: u16,  // instruction index to find signature
    public_key_offset: u16,            // offset to compressed public key of 33 bytes
    public_key_instruction_index: u16, // instruction index to find public key
    message_data_offset: u16,          // offset to start of message data
    message_data_size: u16,            // size of message data
    message_instruction_index: u16,    // index of instruction data to get message data
}

```

The pseudo code of the signature verification:
```
process_instruction() {
    if data.len() < SIGNATURE_OFFSETS_START {
        return Error
    }

    num_signatures = data[0] as usize
    if num_signatures == 0 || num_signatures > 8 {
        return Error
    }

    expected_data_size = num_signatures * SIGNATURE_OFFSETS_SERIALIZED_SIZE + SIGNATURE_OFFSETS_START
    if data.len() < expected_data_size {
        return Error
    }

    for i in 0..num_signatures {
        offsets = parse_signature_offsets(data, i)

        signature = get_data_slice(data, instruction_datas, offsets.signature_instruction_index, offsets.signature_offset, SIGNATURE_SERIALIZED_SIZE)

        if s > half_curve_order {
            return Error
        }

        pubkey = get_data_slice(data, instruction_datas, offsets.public_key_instruction_index, offsets.public_key_offset, COMPRESSED_PUBKEY_SERIALIZED_SIZE)

        message = get_data_slice(data, instruction_datas, offsets.message_instruction_index, offsets.message_data_offset, offsets.message_data_size)

        if !verify_signature(signature, pubkey, message) {
            return Error
        }
    }

    return Success
}
```
Note: Low S values are enforced for all signatures to avoid accidental signature
malleability.

### Optimization notes

The operation will have to take place after (at least partial) deserialization,
but all inputs come from the transaction data itself, which allows it to be
relatively easy to execute in parallel to transaction processing and PoH
verification.
