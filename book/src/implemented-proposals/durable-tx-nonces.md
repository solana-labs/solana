# Durable Transaction Nonces

## Problem

To prevent replay, Solana transactions contain a nonce field populated with a
"recent" blockhash value. A transaction containing a blockhash that is too old
(~2min as of this writing) is rejected by the network as invalid. Unfortunately
certain use cases, such as custodial services, require more time to produce a
signature for the transaction. A mechanism is needed to enable these potentially
offline network participants.

## Requirements

1) The transaction's signature needs to cover the nonce value
2) The nonce must not be reusable, even in the case of signing key disclosure

## A Contract-based Solution

Here we describe a contract-based solution to the problem, whereby a client can
"stash" a nonce value for future use in a transaction's `recent_blockhash`
field. This approach is akin to the Compare and Swap atomic instruction,
implemented by some CPU ISAs.

When making use of a durable nonce, the client must first query its value from
account data. A transaction is now constructed in the normal way, but with the
following additional requirements:

  1) The durable nonce value is used in the `recent_blockhash` field
  2) A `NonceAdvance` instruction is the first issued in the transaction

### Contract Mechanics

TODO: svgbob this into a flowchart

```text
Start
Create Account
  state = Uninitialized
NonceInstruction
  if state == Uninitialized
    if account.balance < rent_exempt
      error InsufficientFunds
    state = Initialized
  elif state != Initialized
    error BadState
  if sysvar.recent_blockhashes.is_empty()
    error EmptyRecentBlockhashes
  if !sysvar.recent_blockhashes.contains(stored_nonce)
    error NotReady
  stored_hash = sysvar.recent_blockhashes[0]
  success
WithdrawInstruction(to, lamports)
  if state == Uninitialized
    if !signers.contains(owner)
      error MissingRequiredSignatures
  elif state == Initialized
    if !sysvar.recent_blockhashes.contains(stored_nonce)
      error NotReady
    if lamports != account.balance && lamports + rent_exempt > account.balance
      error InsufficientFunds
  account.balance -= lamports
  to.balance += lamports
  success
```

A client wishing to use this feature starts by creating a nonce account under 
the system program. This account will be in the `Uninitialized` state with no
stored hash, and thus unusable.

To initialize a newly created account, a `NonceInitialize` instruction must be
issued. This instruction takes one parameter, the `Pubkey` of the account's
[authority](../offline-signing/durable-nonce.md#nonce-authority). Nonce accounts
must be [rent-exempt](rent.md#two-tiered-rent-regime) to meet the data-persistence
requirements of the feature, and as such, require that sufficient lamports be
deposited before they can be initialized. Upon successful initialization, the
cluster's most recent blockhash is stored along with specified nonce authority
`Pubkey`.

The `NonceAdvance` instruction is used to manage the account's stored nonce
value. It stores the cluster's most recent blockhash in the account's state data,
failing if that matches the value already stored there. This check prevents
replaying transactions within the same block.

Due to nonce accounts' [rent-exempt](rent.md#two-tiered-rent-regime) requirement,
a custom withdraw instruction is used to move funds out of the account.
The `NonceWithdraw` instruction takes a single argument, lamports to withdraw,
and enforces rent-exemption by preventing the account's balance from falling
below the rent-exempt minimum. An exception to this check is if the final balance
would be zero lamports, which makes the account eligible for deletion. This
account closure detail has an additional requirement that the stored nonce value
must not match the cluster's most recent blockhash, as per `NonceAdvance`.

The account's [nonce authority](../offline-signing/durable-nonce.md#nonce-authority)
can be changed using the `NonceAuthorize` instruction. It takes one parameter,
the `Pubkey` of the new authority. Executing this instruction grants full
control over the account and its balance to the new authority.

{% hint style="info" %}
`NonceAdvance`, `NonceWithdraw` and `NonceAuthorize` all require the current
[nonce authority](../offline-signing/durable-nonce.md#nonce-authority) for the
account to sign the transaction.
{% endhint %}

### Runtime Support

The contract alone is not sufficient for implementing this feature. To enforce
an extant `recent_blockhash` on the transaction and prevent fee theft via
failed transaction replay, runtime modifications are necessary.

Any transaction failing the usual `check_hash_age` validation will be tested
for a Durable Transaction Nonce. This is signaled by including a `NonceAdvance`
instruction as the first instruction in the transaction.

If the runtime determines that a Durable Transaction Nonce is in use, it will
take the following additional actions to validate the transaction:

  1) The `NonceAccount` specified in the `Nonce` instruction is loaded.
  2) The `NonceState` is deserialized from the `NonceAccount`'s data field and
confirmed to be in the `Initialized` state.
  3) The nonce value stored in the `NonceAccount` is tested to match against the
one specified in the transaction's `recent_blockhash` field.

If all three of the above checks succeed, the transaction is allowed to continue
validation.

Since transactions that fail with an `InstructionError` are charged a fee and
changes to their state rolled back, there is an opportunity for fee theft if a
`NonceAdvance` instruction is reverted. A malicious validator could replay the
failed transaction until the stored nonce is successfully advanced. Runtime
changes prevent this behavior. When a durable nonce transaction fails with an
`InstructionError` aside from the `NonceAdvance` instruction, the nonce account
is rolled back to its pre-execution state as usual. Then the runtime advances
its nonce value and the advanced nonce account stored as if it succeeded.
