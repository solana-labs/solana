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
2) The nonce must not be reusable, even in the face of signing key disclosure

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
  3) The appropriate transaction flag (TBD) is set, signaling that the usual
hash age check should be skipped and the previous requirements enforced

### Contract Mechanics

TODO: svgbob this into a flowchart

```text
Start
Create Account
  state = Uninitialized
InitializeInstruction()
  if !sysvar.recent_blockhashes.is_empty()
    stored_hash = sysvar.recent_blockhashes[0]
    state = Initialized
    success
  else
    error
NonceInstruction(spend_hash) /* spend_hash is a stand in for a TBD sysvar
                                which will replace this "parameter" */
  if spend_hash == stored_hash
    if !sysvar.recent_blockhashes.is_empty()
      stored_hash = sysvar.recent_blockhashes[0]
      success
    else
      error
  else
    error
```

A client wishing to use this feature starts by creating a nonce account and
depositing sufficient lamports as to make it rent-exempt. The resultant account
will be in the `Uninitialized` state with no stored hash and thus unusable.

To begin using the account an `Initialize` instruction is executed on it,
advancing its state to `Initialized` and storing a durable nonce, chosen by the
contract from the `recent_blockhashes` sysvar, in the data field.

For the `Nonce` instruction to succeed:
  1) The nonce account MUST have been advanced to the `Initialized` state,
ensuring that a valid nonce has been stored.
  2) The nonce value in the transaction's `recent_blockhash` field MUST match
the value stored in the nonce account data field. In doing so, the client
commits to the stored nonce value by signing the transaction.
  3) The nonce value MUST NOT reside in the `recent_blockhashes` sysvar, thus
preventing replay by deleting, then recreating the account.
If these requirements are met, the contract replaces the stored nonce with a
new one from the current values in the `recent_blockhashes` sysvar.

To discard a nonce account, the client should include a `Nonce` instruction in
a transaction which withdraws all lamports, leaving a zero balance and making
it eligible for deletion.

### Runtime Support

The contract alone is not sufficient for implementing this feature, as an extant
`recent_blockhash` is still required on any transaction executing the `Nonce`
instruction. To alleviate this, a `flags` field will be added to the transaction
message and one bit reserved to signal the use of a Durable Transaction Nonce,
which skips the typical age check.

### Open Questions

* Should this feature be restricted in the number of uses per transaction?
