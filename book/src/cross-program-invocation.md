# Cross-Program Invocation

## Problem

In today's implementation a client can create a transaction that modifies two
accounts, each owned by a separate on-chain program:

```rust,ignore
let message = Message::new(vec![
    foo_instruction::foo(&alice_pubkey),
    bar_instruction::bar(&bob_pubkey),
]);
client.send_message(&[&alice_keypair, &bob_keypair], &message);
```

The current implementation does not, however, allow the `bar` program to
conveniently invoke `foo` instructions on the client's behalf:

```rust,ignore
let message = Message::new(vec![
    bar_instruction::foobar(&alice_pubkey, &bob_pubkey),
]);
client.send_message(&[&alice_keypair, &bob_keypair], &message);
```

Currently, there is no way to create instruction `foobar` that executes executes
`foo_instruction::foo` from the `bar` program. The workaround is to extend the
`bar` program with the implementation of the `foo` program, and create `foo`
accounts with `BAR_PROGRAM_ID`, which the `bar` program is permitted to modify.
With that workaround, `bar` can modify foo-like accounts created by the `bar`
program, but not foo accounts created by the `foo` program.


## Proposed Solution

The goal of this design is to modify Solana's runtime such that an on-chain
program can invoke an instruction from another program.

Given two on-chain programs `foo` and `bar`, each implementing instructions
`foo` and `bar` respectively, we would like to implement bar as close as
possible to as the following Rust code of a cross-module function call:

```rust,ignore
use foo;

fn bar(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    ...
}

fn foobar(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    foo::foo(&keyed_accounts[1..])?;

    bar(keyed_accounts)?;
}
```

The above code would require that the `foo` crate be dynamically
linked, so that a custom linker could could intercept calls and
validate accesses to `keyed_accounts`. That is, even though the client
intends to modify both `foo` and `bar` accounts, only `foo` program is
permitted to modify the `foo` account, and only the `bar` program is
permitted to modify the `bar` account.

Backing off from that ideal cross-program call, a slightly more
verbose solution to is expose foo's existing `process_instruction()`
entrypoint to the bar program:

```rust,ignore
use foo_instruction;

fn bar(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    ...
}

fn foobar(keyed_accounts: &[KeyedAccount]) -> Result<()> {
    let alice_pubkey = keyed_accounts[1].key;
    let instruction = foo_instruction::foo(&alice_pubkey);
    process_instruction(&instruction)?;

    bar(keyed_accounts)?;
}
```

where `process_instruction()` is built into Solana's runtime and responsible
for routing the given instruction to the `foo` program via the instruction's
`program_id` field. Before invoking `foo()`, the runtime must also ensure that
`bar` didn't modify any accounts owned by `foo`. It does this by calling
`runtime::verify_instruction()` and then afterward updating all the `pre_*`
variables to tentatively commit `bar`'s account modifications. After `foo()`
completes, the runtime must again ensure that `foo` didn't modify any accounts
owned by `bar`. It should call `verify_instruction()` again, but this time with
the `foo` program ID. Lastly, after `foobar()` completes, the runtime must call
`verify_instruction()` one more time, where it normally would, but using all
updated `pre_*` variables.  If `foobar()` up to `foo()` makes no invalid
changes, then `foo()` makes no invalid changes, then the code from `foo()`
until `foobar()` returns makes no invalid changes, the runtime can transitively
assume `foobar()` as whole made no invalid changes, and commit all account
modifications.

### Setting `KeyedAccount.is_signer`

When `process_instruction()` is invoked, the runtime must create a new
`KeyedAccounts` parameter using the signatures from the *original* transaction
data. Since the `foo` program is immutable and existed on-chain prior to the
`bar` program, the runtime can safely treat the transaction signature as a
signature of a transaction with a `foo` instruction. When the runtime sees the
given instruction references `alice_pubkey`, it looks up the key in the
transaction to see if that key corresponds to a transaction signature. In this
case it does and so sets `KeyedAccount.is_signer`, thereby authorizing the
`foo` program to modify Alice's account.
