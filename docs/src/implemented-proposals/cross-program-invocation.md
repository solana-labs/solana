# Cross-Program Invocation

## Problem

In today's implementation, a client can create a transaction that modifies two accounts, each owned by a separate on-chain program:

```rust,ignore
let message = Message::new(vec![
    token_instruction::pay(&alice_pubkey),
    acme_instruction::launch_missiles(&bob_pubkey),
]);
client.send_message(&[&alice_keypair, &bob_keypair], &message);
```

However, the current implementation does not allow the `acme` program to conveniently invoke `token` instructions on the client's behalf:

```rust,ignore
let message = Message::new(vec![
    acme_instruction::pay_and_launch_missiles(&alice_pubkey, &bob_pubkey),
]);
client.send_message(&[&alice_keypair, &bob_keypair], &message);
```

Currently, there is no way to create instruction `pay_and_launch_missiles` that executes `token_instruction::pay` from the `acme` program. A possible workaround is to extend the `acme` program with the implementation of the `token` program and create `token` accounts with `ACME_PROGRAM_ID`, which the `acme` program is permitted to modify. With that workaround, `acme` can modify token-like accounts created by the `acme` program, but not token accounts created by the `token` program.

## Proposed Solution

The goal of this design is to modify Solana's runtime such that an on-chain program can invoke an instruction from another program.

Given two on-chain programs `token` and `acme`, each implementing instructions `pay()` and `launch_missiles()` respectively, we would ideally like to implement the `acme` module with a call to a function defined in the `token` module:

```rust,ignore
mod acme {
    use token;

    fn launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
        ...
    }

    fn pay_and_launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
        token::pay(&keyed_accounts[1..])?;

        launch_missiles(keyed_accounts)?;
    }
```

The above code would require that the `token` crate be dynamically linked so that a custom linker could intercept calls and validate accesses to `keyed_accounts`. Even though the client intends to modify both `token` and `acme` accounts, only `token` program is permitted to modify the `token` account, and only the `acme` program is allowed to modify the `acme` account.

Backing off from that ideal direct cross-program call, a slightly more verbose solution is to allow `acme` to invoke `token` by issuing a token instruction via the runtime.

```rust,ignore
mod acme {
    use token_instruction;

    fn launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
        ...
    }

    fn pay_and_launch_missiles(keyed_accounts: &[KeyedAccount]) -> Result<()> {
        let alice_pubkey = keyed_accounts[1].key;
        let instruction = token_instruction::pay(&alice_pubkey);
        invoke(&instruction, accounts)?;

        launch_missiles(keyed_accounts)?;
    }
```

`invoke()` is built into Solana's runtime and is responsible for routing the given instruction to the `token` program via the instruction's `program_id` field.

Before invoking `pay()`, the runtime must ensure that `acme` didn't modify any accounts owned by `token`.  It does this by applying the runtime's policy to the current state of the accounts at the time `acme` calls `invoke` vs. the initial state of the accounts at the beginning of the `acme`'s instruction.  After `pay()` completes, the runtime must again ensure that `token` didn't modify any accounts owned by `acme` by again applying the runtime's policy, but this time with the `token` program ID.  Lastly, after `pay_and_launch_missiles()` completes, the runtime must apply the runtime policy one more time, where it normally would, but using all updated `pre_*` variables. If executing `pay_and_launch_missiles()` up to `pay()` made no invalid account changes, `pay()` made no invalid changes, and executing from `pay()` until `pay_and_launch_missiles()` returns made no invalid changes, then the runtime can transitively assume `pay_and_launch_missiles()` as whole made no invalid account changes, and therefore commit all these account modifications.

### Instructions that require privileges

The runtime uses the privileges granted to the caller program to determine what privileges can be extended to the callee.  Privileges in this context refer to signers and writable accounts.  For example, if the instruction the caller is processing contains a signer or writable account, then the caller can invoke an instruction that also contains that signer and/or writable account.

This privilege extension relies on the fact that programs are immutable.  In the case of the `acme` program, the runtime can safely treat the transaction's signature as a signature of a `token` instruction.  When the runtime sees the `token` instruction references `alice_pubkey`, it looks up the key in the `acme` instruction to see if that key corresponds to a signed account. In this case, it does and thereby authorizes the `token` program to modify Alice's account.

### Program signed accounts

Programs can issue instructions that contain signed accounts that were not signed in the original transaction by
using [Program derived addresses](program-derived-addresses.md).

To sign an account with program derived addresses, a program may `invoke_signed()`.

```rust,ignore
        invoke_signed(
            &instruction,
            accounts,
            &[&["First addresses seed"], 
              &["Second addresses first seed", "Second addresses second seed"]],
        )?;
```

### Reentrancy

Reentrancy is currently limited to direct self recursion capped at a fixed depth.  This restriction prevents situations where a program might invoke another from an intermediary state without the knowledge that it might later be called back into.  Direct recursion gives the program full control of its state at the point that it gets called back.
