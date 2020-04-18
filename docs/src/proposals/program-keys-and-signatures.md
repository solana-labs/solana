# Program Keys and Signatures

## Problem

Programs cannot generate their own signatures in `process_instruction`
as defined in the [Cross-Program Invocations](cross-program-invocation.md)
design.

Lack of programmatic signature generation limits the kinds of programs
that can be implemented in Solana.  For example, a program cannot take
ownership of a TokenAccount and later in a different transaction transfer
the ownership based on the state of another program.  If two users want
to make a wager in tokens on the outcome of a game in Solana, they must
transfer tokens to some intermediary that will honor their agreement.
Currently there is no way to implement this intermediary as a program
in Solana.

This capability is necessary for many DeFi applications, since they
require assets to be transferred to an escrow agent until some event
occurs that determines the new owner.

* Decentralized Exchanges that transfer assets between matching bid and
ask orders.

* Auctions that transfer assets to the winner.

* Games or prediction markets that collect and redistribute prizes to
the winners.

## Proposed Solution

The key to the design is two fold:

1. Allow programs to control specific addresses, called Program
Addresses, in such a way that it is impossible for any external
user to generate valid transactions with signatures for those
addresses.

2. To allow programs to programatically control
`KeyedAccount::is_signer` value for Program Addresses that are
present in instructions that is invoked via `process_instruction()`.

Given the two conditions, users can securely transfer or assign
ownershp of on chain assets to Program Addresses.  Once assigned,
the program and only the program can execute instructions that
refences a Program Address with `KeyedAccount::is_signer` set to
true.

### Private keys for Program Addresses

This address has no private key associated with it, and generating
a signature for it is impossible.  While it has no private key of
its own, the program can issue an instruction to set the
`KeyedAccount::is_signer` flag for this address.

### Hash based generated Program Addresses

All 256 bit values are valid ed25519 curve points, and valid ed25519 public
keys.  All are equally secure and equally as hard to break.
Based on this assumption, Program Addresses can be deterministically
derived from a base seed using a 256 bit preimage resistant hash function.

Deterministic Program Addresses for programs follow a similar derivation
path as Accounts created with `SystemInstruction::CreateAccountWithSeed`
which is implemented with `system_instruction::create_address_with_seed`.

For reference the implementation is as follows:

```rust,ignore
pub fn create_address_with_seed(
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}
```

Programs can deterministically derive any number of addresses by
using a keyword.  The keyword can symbolically identify how this
address is used.

```rust,ignore
//! Generate a derived program address
//!     * program_id, the program's id
//!     * key_base, can be any public key chosen by the program
//!     * keyword, symbolic keyword to identify the key
//!
//! The tuple (`key_base`, `keyword`) is used by programs to create user specific
//! symbolic keys.  For example for the staking contact, the program may need:
//!     * <user account>/<"withdrawer">
//!     * <user account>/<"staker">
//!     * <user account>/<"custodian">
//! As generated keys to control a single stake account for each user.
pub fn derive_program_address(
    program_id: &Pubkey,
    key_base, &Pubkey,
    keyword, &str,
) -> Result<Pubkey, SystemError> {

    // Generate a deterministic base for all program addresses that
    // are owned by `program_id`.
    // Hashing twice is recommended to prevent lenght extension attacks.
    Ok(Pubkey::new(
        hashv(&[hashv(&[program_id.as_ref(), key_base.as_ref(), keyword.as_ref(),
            &"ProgramAddress11111111111111111111111111111"]).as_ref()])
    ))
}
```

### Using Program Addresses

Clients can use the `derive_program_address` function to generate
a destination address.

```rust,ignore
//deterministically derive the escrow key
let escrow_pubkey = derive_program_address(&escrow_program_id, &alice_pubkey, &"escrow");
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);
//transfer 1 token to escrow
client.send_message(&[&alice_keypair], &message);
```

Programs can use the same function to generate the same address.
Below the program issue a `token_instruction::transfer` from its
own address as if it had a private key to sign the transaction.

```rust,ignore
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {


    //user supplies the destination
    let alice_pubkey = keyed_accounts[1].key;

    // Deterministically derive the escrow pubkey.
    let escrow_pubkey = derive_program_address(program_id, &alice_pubkey, &"escrow");

    //create the transfer instruction
    let instruction = token_instruction::transfer(&escrow_pubkey, &alice_pubkey, 1);

    // The runtime deterministically derives the key from the current
    // program id and the supplied keywords.
    // If the derived key matches a key in the instruction
    // the `is_signed` flag is set.
    process_signed_instruction(&instruction,  &[(&alice_pubkey, &"escrow")])?
}
```

### Setting `KeyedAccount::is_signer`

The addresses generated with `derive_program_address` are blinded
and are indistinguishable from any other pubkey.  The only way for
the runtime to verify that the address belongs to a program is for
the program to supply the keyword used to generate the address.

The runtime will internally run  `derive_program_address(program_id,
&alice_pubkey, &"escrow")`, and compare the result against the addresses
supplied in the instruction.
