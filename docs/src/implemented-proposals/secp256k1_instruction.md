---
title: secp256k1 builtin instruction
---

## Problem

Performing multiple secp256k1 pubkey recovery operations (ecrecover) in BPF would exceed the transction bpf instruction
limit and even if the limit is increased it would take a long time to process.
ecrecover is an ethereum instruction which takes a signature and message and recovers a publickey, a comparison
to that public key can thus verify that the signature is valid.

Since there needs to be 10-20 signatures in the transaction as well as the signing data which is on the
order of 500 bytes, transaction space is a concern. But also having more concentrated similar work should
provide for easier optimization.

## Solution

Add a new builtin instruction which takes in as the first byte a count of the following struct serialized in the instruction
data:

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

This allows the user to specify any instruction data in the transaction for signature and message data.
By specifying a special instructions sysvar, one can also receive data from the transaction itself.

Cost of the transaction will count the number of signatures to verify multiplied by the signature cost verify multiplier.

## Optimization notes

The operation will have to take place after (at least partial) deserialization, but all inputs come
from the transaction data itself, this allows it to be relatively easy to execute in parallel to
transaction processing and PoH verification.

## Other solutions

* Instruction available as CPI such that the program can call as desired or a syscall which can operate on the instruction inline.
   - Could be harder to optimize given that it generally either requires bpf program scan to determine the inputs to the operation,
     or the implementation needs to just wait until the program hits the operation in bpf processing to evaluate it.
   - Vector version of the operation could allow for somewhat efficient simd/gpu execution. For most efficient though,
     batching with other instructions in the pipeline would be ideal.
   - Pros - Nicer interface for the user.

* Async execution environment inside bpf
   - Might be hard to optimize for devices like gpus which cannot queue work for itself easily
   - Might be easier to optimize on cpu since ordering can be more explicit

* All inputs have to come from the instruction
   - Pros - easier to optimize, data is already sent to the GPU for instance for regular sigverify. Probably still need to
     wait for deserialize though.
   - Cons - ask for pubkeys outside the transaction data itself since they would not be stored on the transaction sending client,
     and larger transaction size.
