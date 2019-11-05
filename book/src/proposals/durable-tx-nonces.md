# Durable Transaction Nonces

## Problem

To prevent replay, Solana transactions contain a nonce field populated with a
"recent" blockhash value. A transaction containing a blockhash that is too old
(~2min as of this writing) is rejected by the network as invalid. Unfortunately
certain use cases, such as custodial services, require more time to produce a
signature for the transaction. A mechanism is needed to enable these potentially
offline network participants.

## A Naive Solution

An initial attempt at this was made by designating an special blockhash value
that, if present in the `recent_blockhash` field, allowed the transaction to
succeed so long as it additionally spent the full balance of the fee account.
However, this would introduce a security vulnerability by the fact that the
transaction could be replayed by simply funding the fee account again.

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
InitializeInstruction(hash)
  if hash is recent
    stored_hash = hash
    state = Initialized
    success
  else
    error
SpendInstruction(spend_hash, next_hash)
  if spend_hash == stored_hash
    if next_hash is recent
      stored_hash = next_hash
      success
    else
      error
  else
    error
```

### Runtime Support

The contract alone is not sufficient for implementing this feature, as an extant
`recent_blockhash` is still required on any transaction executing the `Spend`
instruction. To alleviate this, a `flags` field will be added to the transaction
message and one bit reserved to signal the use of a Durable Transaction Nonce,
which skips the typical age check.

### Open Questions

* Should an instruction be added to explicitly drive the account state back to
`Uninitialized` or just leave it subject to rent?

* Should this feature be restricted in the number of uses per transaction?
