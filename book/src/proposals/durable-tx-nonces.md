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
  2) A `Nonce` instruction is issued (first?)
  3) The appropriate transaction flag is set, signaling that the usual
hash age check should be skipped and the previous requirements enforced. This
may be unnecessary, see [Runtime Support](#runtime-support) below

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

A client wishing to use this feature starts by creating a nonce account and
depositing sufficient lamports as to make it rent-exempt. The resultant account
will be in the `Uninitialized` state with no stored hash and thus unusable.

The `Nonce` instruction is used to request that a new nonce be stored for the
calling account. The first `Nonce` instruction run on a newly created account
will drive the account's state to `Initialized`. As such, a `Nonce` instruction
MUST be issued before the account can be used.

To discard a `NonceAccount`, the client should issue a `Withdraw` instruction
which withdraws all lamports, leaving a zero balance and making the account
eligible for deletion.

`Nonce` and `Withdraw` instructions each will only succeed if the stored
blockhash is no longer resident in sysvar.recent_blockhashes.

### Runtime Support

The contract alone is not sufficient for implementing this feature. To enforce
an extant `recent_blockhash` on the transaction and prevent fee theft via
failed transaction replay, runtime modifications are necessary.

Any transaction failing the usual `check_hash_age` validation will be tested
for a Durable Transaction Nonce. This specifics of this test are undecided, some
options:

  1) Require that the `Nonce` instruction be the first in the transaction
    * + No ABI changes
    * + Fast and simple
    * - Sets a precedent that may lead to incompatible instruction combinations
  2) Blind search for a `Nonce` instruction over all instructions in the
transaction
    * + No ABI changes
    * - Potentially slow
  3) [2], but guarded by a transaction flag
    * - ABI changes
    * - Wire size increase
    * + We'll probably end up with some sort of flags eventually anyway

Current prototyping will use [1]. If it is determined that a Durable Transaction
Nonce is in use, the runtime will take the following actions to validate the
transaction:

  1) The `NonceAccount` specified in the `Nonce` instruction is loaded.
  2) The `NonceState` is deserialized from the `NonceAccount`'s data field and
confirmed to be in the `Initialized` state.
  3) The nonce value stored in the `NonceAccount` is tested to match against the
one specified in the transaction's `recent_blockhash` field.

If all three of the above checks succeed, the transaction is allowed to continue
validation.

### Open Questions

* Should this feature be restricted in the number of uses per transaction?
