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

### Contract Mechanics

TODO: svgbob this into a flowchart, add prose

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

### Runtime Support

The contract alone is not sufficient for implementing this feature, as an extant
`recent_blockhash` is still required on any transaction executing the `Nonce`
instruction. To alleviate this, a `flags` field will be added to the transaction
message and one bit reserved to signal the use of a Durable Transaction Nonce,
which skips the typical age check.

### Open Questions

* Should this feature be restricted in the number of uses per transaction?
