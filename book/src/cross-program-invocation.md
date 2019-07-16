# Cross-Program Invocation

## Problem

In today's implementation a client can create a transaction that modifies two
accounts, each owned by a separate on-chain program:

```rust,ignore
let message = Message::new(vec![
    token_instruction::pay(&alice_pubkey),
    acme_instruction::launch_missiles(&bob_pubkey),
]);
client.send_message(&[&alice_keypair, &bob_keypair], &message);
```

The current implementation does not, however, allow the `acme` program to
conveniently invoke `token` instructions on the client's behalf:

```rust,ignore
let message = Message::new(vec![
    acme_instruction::pay_and_launch_missiles(&alice_pubkey, &bob_pubkey),
]);
client.send_message(&[&alice_keypair, &bob_keypair], &message);
```

Currently, there is no way to create instruction `pay_and_launch_missiles` that executes
`token_instruction::pay` from the `acme` program. The workaround is to extend the
`acme` program with the implementation of the `token` program, and create `token`
accounts with `ACME_PROGRAM_ID`, which the `acme` program is permitted to modify.
With that workaround, `acme` can modify token-like accounts created by the `acme`
program, but not token accounts created by the `token` program.


## Proposed Solution

The goal of this design is to modify Solana's runtime such that an on-chain
program can invoke an instruction from another program.

Given two on-chain programs `token` and `acme`, each implementing instructions
`pay()` and `launch_missiles()` respectively, we would ideally like to implement
the `acme` module with a call to a function defined in the `token` module:

```rust,ignore
use token;

fn launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    ...
}

fn pay_and_launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    token::pay(&keyed_accounts[1..])?;

    launch_missiles(keyed_accounts)?;
}
```

The above code would require that the `token` crate be dynamically linked,
so that a custom linker could intercept calls and validate accesses to
`keyed_accounts`. That is, even though the client intends to modify both
`token` and `acme` accounts, only `token` program is permitted to modify
the `token` account, and only the `acme` program is permitted to modify
the `acme` account.

Backing off from that ideal cross-program call, a slightly more
verbose solution is to expose token's existing `process_instruction()`
entrypoint to the acme program:

```rust,ignore
use token_instruction;

fn launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    ...
}

fn pay_and_launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    let alice_pubkey = keyed_accounts[1].key;
    let instruction = token_instruction::pay(&alice_pubkey);
    process_instruction(&instruction)?;

    launch_missiles(keyed_accounts)?;
}
```

where `process_instruction()` is built into Solana's runtime and responsible
for routing the given instruction to the `token` program via the instruction's
`program_id` field. Before invoking `pay()`, the runtime must also ensure that
`acme` didn't modify any accounts owned by `token`. It does this by calling
`runtime::verify_instruction()` and then afterward updating all the `pre_*`
variables to tentatively commit `acme`'s account modifications. After `pay()`
completes, the runtime must again ensure that `token` didn't modify any
accounts owned by `acme`. It should call `verify_instruction()` again, but this
time with the `token` program ID. Lastly, after `pay_and_launch_missiles()`
completes, the runtime must call `verify_instruction()` one more time, where it
normally would, but using all updated `pre_*` variables.  If executing
`pay_and_launch_missiles()` up to `pay()` made no invalid account changes,
`pay()` made no invalid changes, and executing from `pay()` until
`pay_and_launch_missiles()` returns made no invalid changes, then the runtime
can transitively assume `pay_and_launch_missiles()` as whole made no invalid
account changes, and therefore commit all account modifications.

### Setting `KeyedAccount.is_signer`

When `process_instruction()` is invoked, the runtime must create a new
`KeyedAccounts` parameter using the signatures from the *original* transaction
data. Since the `token` program is immutable and existed on-chain prior to the
`acme` program, the runtime can safely treat the transaction signature as a
signature of a transaction with a `token` instruction. When the runtime sees
the given instruction references `alice_pubkey`, it looks up the key in the
transaction to see if that key corresponds to a transaction signature. In this
case it does and so sets `KeyedAccount.is_signer`, thereby authorizing the
`token` program to modify Alice's account.
