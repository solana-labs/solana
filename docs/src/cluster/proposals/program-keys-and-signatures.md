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

The goal of this design is to allow programs to generate deterministic
program specific addresses and programmatically set
`KeyedAccount.is_signer` for those addresses when `process_instruction()`
is invoked.

### Deterministic Pubkey Addresses for Programs

Deterministic pubkey addresses for programs follow a similar derivation
path as Accounts created with `SystemInstruction::CreateAccountWithSeed`.

```text

use system_instruction::create_address_with_seed;

pub fn derive_program_address(
    program_id: &Pubkey,
    keyword, &str,
) -> Result<Pubkey, SystemError> {

    //generate a deterministic base for all program addresses
    let base = create_address_with_seed(program_id,
            &"derived program base address" , program_id)?;

    //generate the keyword specific address
    create_address_with_seed(&base, keyword, program_id);
}
```

These addresses are hashed twice, and are impossible to generate
externally with `CreateAccountWithSeed` or via other means.

### Using Program Keys 

Clients can use the `derive_program_address` function to generate
keys.

```text
//deterministically derive the escrow key
let escrow = derive_program_address(&escrow_program_id, &"escrow");
let message = Message::new(vec![
    token_instruction::transfer(&alice, &escrow, 1),
]);
//transfer 1 token to escrow
client.send_message(&[&alice, &escrow], &message);
```

Escrow program can issue a `token_instruction::transfer` from its own
address as if it had a private key to sign the transaction.

```text
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {

    //deterministically derive the escrow key
    let escrow = derive_program_address(program_id, &"escrow");

    //user supplies the destination
    let alice_pubkey = keyed_accounts[1].key;

    //create the transfer instruction
    let instruction = token_instruction::transfer(&escrow, &alice, 1);

    //Sign it with the key keyword.
    //The runtime deterministically derives the key from the current
    //program id and the keyword.
    //If the derived key match a key in the instrution,
    //the `is_signed` flag is set.
    sign_instruction(&instruction, &"escrow")?;

    process_instruction(&instruction)?;
}
```
