---
title: Program Derived Addresses
---

## Problem

Programs cannot generate signatures when issuing instructions to other programs
as defined in the [Cross-Program Invocations](cpi.md)
design.

The lack of programmatic signature generation limits the kinds of programs that
can be implemented in Solana. A program may be given the authority over an
account and later want to transfer that authority to another. This is impossible
today because the program cannot act as the signer in the transaction that gives
authority.

For example, if two users want to make a wager on the outcome of a game in
Solana, they must each transfer their wager's assets to some intermediary that
will honor their agreement. Currently, there is no way to implement this
intermediary as a program in Solana because the intermediary program cannot
transfer the assets to the winner.

This capability is necessary for many DeFi applications since they require
assets to be transferred to an escrow agent until some event occurs that
determines the new owner.

- Decentralized Exchanges that transfer assets between matching bid and ask
  orders.

- Auctions that transfer assets to the winner.

- Games or prediction markets that collect and redistribute prizes to the
  winners.

## Solution

The key to the design is two-fold:

1. Allow programs to control specific addresses, called program addresses, in
   such a way that no external user can generate valid transactions with
   signatures for those addresses.

2. Allow programs to programmatically sign for programa addresses that are
   present in instructions invoked via [Cross-Program
   Invocations](cpi.md).

Given the two conditions, users can securely transfer or assign the authority of
on-chain assets to program addresses and the program can then assign that
authority elsewhere at its discretion.

### Private keys for program addresses

A Program address does not lie on the ed25519 curve and therefore has no valid
private key associated with it, and thus generating a signature for it is
impossible.  While it has no private key of its own, it can be used by a program
to issue an instruction that includes the Program address as a signer.

### Hash-based generated program addresses

Program addresses are deterministically derived from a collection of seeds and a
program id using a 256-bit pre-image resistant hash function.  Program address
must not lie on the ed25519 curve to ensure there is no associated private key.
During generation an error will be returned if the address is found to lie on
the curve.  There is about a 50/50 change of this happening for a given
collection of seeds and program id.  If this occurs a different set of seeds or
a seed bump (additional 8 bit seed) can be used to find a valid program address
off the curve.

Deterministic program addresses for programs follow a similar derivation path as
Accounts created with `SystemInstruction::CreateAccountWithSeed` which is
implemented with `system_instruction::create_address_with_seed`.

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

Programs can deterministically derive any number of addresses by using seeds.
These seeds can symbolically identify how the addresses are used.

From `Pubkey`::

```rust,ignore
/// Generate a derived program address
///     * seeds, symbolic keywords used to derive the key
///     * program_id, program that the address is derived for
pub fn create_program_address(
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<Pubkey, PubkeyError>
```

### Using program addresses

Clients can use the `create_program_address` function to generate a destination
address.

```rust,ignore
// deterministically derive the escrow key
let escrow_pubkey = create_program_address(&[&["escrow"]], &escrow_program_id);

// construct a transfer message using that key
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);

// process the message which transfer one 1 token to the escrow
client.send_and_confirm_message(&[&alice_keypair], &message);
```

Programs can use the same function to generate the same address. In the function
below the program issues a `token_instruction::transfer` from a program address
as if it had the private key to sign the transaction.

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
    // If the derived address matches a key marked as signed in the instruction
    // then that key is accepted as signed.
    invoke_signed(&instruction,  &[&["escrow"]])?
}
```

### Instructions that require signers

The addresses generated with `create_program_address` are indistinguishable from
any other public key. The only way for the runtime to verify that the address
belongs to a program is for the program to supply the seeds used to generate the
address.

The runtime will internally call `create_program_address`, and compare the
result against the addresses supplied in the instruction.
