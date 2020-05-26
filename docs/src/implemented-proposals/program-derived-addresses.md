# Program Derived Addresses

## Problem

Programs cannot generate signatures when issuing instructions to
other programs as defined in the [Cross-Program Invocations](cross-program-invocation.md)
design.

The lack of programmatic signature generation limits the kinds of programs
that can be implemented in Solana.  A program may be given the
authority over an account and later want to transfer that authority to another.
This is impossible today because the program cannot act as the signer in the transaction that gives authority.

For example, if two users want
to make a wager on the outcome of a game in Solana, they must each
transfer their wager's assets to some intermediary that will honor
their agreement.  Currently, there is no way to implement this intermediary
as a program in Solana because the intermediary program cannot transfer the
assets to the winner.

This capability is necessary for many DeFi applications since they
require assets to be transferred to an escrow agent until some event
occurs that determines the new owner.

* Decentralized Exchanges that transfer assets between matching bid and
ask orders.

* Auctions that transfer assets to the winner.

* Games or prediction markets that collect and redistribute prizes to
the winners.

## Proposed Solution

The key to the design is two-fold:

1. Allow programs to control specific addresses, called Program-Addresses, in such a way that no external
user can generate valid transactions with signatures for those
addresses.

2. Allow programs to programmatically sign for Program-Addresses that are
present in instructions invoked via [Cross-Program Invocations](cross-program-invocation.md).

Given the two conditions, users can securely transfer or assign
the authority of on-chain assets to Program-Addresses and the program
can then assign that authority elsewhere at its discretion.

### Private keys for Program Addresses

A Program -Address has no private key associated with it, and generating
a signature for it is impossible.  While it has no private key of
its own, it can issue an instruction that includes the Program-Address as a signer.

### Hash-based generated Program Addresses

All 256-bit values are valid ed25519 curve points and valid ed25519 public
keys.  All are equally secure and equally as hard to break.
Based on this assumption, Program Addresses can be deterministically
derived from a base seed using a 256-bit preimage resistant hash function.

Deterministic Program Addresses for programs follow a similar derivation
path as Accounts created with `SystemInstruction::CreateAccountWithSeed`
which is implemented with `system_instruction::create_address_with_seed`.

For reference that implementation is as follows:

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
using keywords.  These keywords can symbolically identify how the addresses are used.

```rust,ignore
//! Generate a derived program address
//!     * seeds, symbolic keywords used to derive the key
//!     * owner, program that the key is derived for
pub fn create_program_address(seeds: &[&str], owner: &Pubkey) -> Result<Pubkey, PubkeyError> {
  let mut hasher = Hasher::default();
  for seed in seeds.iter() {
      if seed.len() > MAX_SEED_LEN {
          return Err(PubkeyError::MaxSeedLengthExceeded);
      }
      hasher.hash(seed.as_ref());
  }
  hasher.hashv(&[owner.as_ref(), "ProgramDerivedAddress".as_ref()]);

  Ok(Pubkey::new(hashv(&[hasher.result().as_ref()]).as_ref()))
}
```

### Using Program Addresses

Clients can use the `create_program_address` function to generate
a destination address.

```rust,ignore
//deterministically derive the escrow key
let escrow_pubkey = create_program_address(&[&["escrow"]], &escrow_program_id);
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);
//transfer 1 token to escrow
client.send_message(&[&alice_keypair], &message);
```

Programs can use the same function to generate the same address.
In the function below the program issues a `token_instruction::transfer` from
Program Address as if it had the private key to sign the transaction.

```rust,ignore
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {

    // User supplies the destination
    let alice_pubkey = keyed_accounts[1].unsigned_key();

    // Deterministically derive the escrow pubkey.
    let escrow_pubkey = create_program_address(&[&["escrow"]], program_id);

    // Create the transfer instruction
    let instruction = token_instruction::transfer(&escrow_pubkey, &alice_pubkey, 1);

    // The runtime deterministically derives the key from the currently
    // executing program ID and the supplied keywords.
    // If the derived key matches a key marked as signed in the instruction
    // then that key is accepted as signed.
    invoke_signed(&instruction,  &[&["escrow"]])?
}
```

### Instructions that require signers

The addresses generated with `create_program_address` are indistinguishable
from any other public key.  The only way for the runtime to verify that the
address belongs to a program is for the program to supply the keywords used
to generate the address.

The runtime will internally call `create_program_address`, and compare the
result against the addresses supplied in the instruction.