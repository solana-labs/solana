---
title: "Transactions"
---

Program execution begins with a [transaction](terminology.md#transaction) being
submitted to the cluster. The Solana runtime will execute a program to process
each of the [instructions](terminology.md#instruction) contained in the
transaction, in order, and atomically.

See [Anatomy of a Transaction](transaction.md) for more information about how a
transaction is encoded.

## Instructions

Each [instruction](terminology.md#instruction) specifies a single program, a
subset of the transaction's accounts that should be passed to the program, and a
data byte array that is passed to the program. The program interprets the data
array and operates on the accounts specified by the instructions. The program
can return successfully, or with an error code. An error return causes the
entire transaction to fail immediately.

Program's typically provide helper functions to construct instruction they
support. For example, the system program provides the following Rust helper to
construct a
[`SystemInstruction::CreateAccount`](https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L63)
instruction:

```rust
pub fn create_account(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, true),
    ];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::CreateAccount {
            lamports,
            space,
            owner: *owner,
        },
        account_metas,
    )
}
```

Which can be found here:

https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L220

### Program ID

The instruction's [program id](terminology.md#program-id) specifies which
program will process this instruction. The program's account data contains
information about how the runtime should execute the program, in the case of BPF
programs, the account data holds the BPF bytecode. Program accounts are marked
as executable once they are successfully deployed. The runtime will reject
transactions that specify programs that are not executable.

### Accounts

The accounts referenced by an instruction represent on-chain state and serve as
both the inputs and outputs of a program. More information about Accounts can be
found in the [Accounts](accounts.md) section.

### Instruction data

Each instruction caries a general purpose byte array that is passed to the
program along with the accounts. The contents of the instruction data is program
specific and typically used to convey what operations the program should
perform, and any additional information those operations may need above and
beyond what the accounts contain.

Programs are free to specify how information is encoded into the instruction
data byte array. The choice of how data is encoded should take into account the
overhead of decoding since that step is performed by the program on-chain. It's
been observed that some common encodings (Rust's bincode for example) are very
inefficient.

The [Solana Program Library's Token
program](https://github.com/solana-labs/solana-program-library/tree/master/token)
gives one example of how instruction data can be encoded efficiently, but note
that this method only supports fixed sized types. Token utilizes the
[Pack](https://github.com/solana-labs/solana/blob/master/sdk/program/src/program_pack.rs)
trait to encode/decode instruction data for both token instructions as well as
token account states.

## Signatures

Each transaction explicitly lists all account public keys referenced by the
transaction's instructions. A subset of those public keys are each accompanied
by a transaction signature. Those signatures signal on-chain programs that the
account holder has authorized the transaction. Typically, the program uses the
authorization to permit debiting the account or modifying its data. More
information about how the authorization is communicated to a program can be
found in [Accounts](accounts.md#signers)


## Recent Blockhash

A transaction includes a recent [blockhash](terminology.md#blockhash) to prevent
duplication and to give transactions lifetimes. Any transaction that is
completely identical to a previous one is rejected, so adding a newer blockhash
allows multiple transactions to repeat the exact same action. Transactions also
have lifetimes that are defined by the blockhash, as any transaction whose
blockhash is too old will be rejected.
