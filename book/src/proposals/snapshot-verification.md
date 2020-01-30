# Snapshot Verification

## Problem

When a validator boots up from a snapshot, it needs a way to verify the account set matches what the rest of the network sees quickly. A potential
attacker could give the validator an incorrect state, and then try to convince it to accept a transaction that would otherwise be rejected.

## Solution

Propose to use an RSA accumulator where it hashes the account state to a large prime-ish number
using a primality test and incrementing a nonce to find a hash that passes the primality test.

On account store of non-zero lamport accounts, we hash the following data:

* Account owner
* Account data
* Account pubkey
* Account lamports balance
* Fork the account is stored on

That hash is added to an RSA accumulator in standard `g^(hash) mod n` way.

Since adding to an accumulator is a single-threaded process, multiple accumulators can
be used to obtain parallelism for the entire account set in the slot. They can be indexed some
bits of the hash\_to\_prime(account\_state) result.

The state that was replaced is added to the 'deleted' set of accumulators.

The bank hash will then hash the final state of the 'add' and 'deleted' accumulators and include it in the bank hash.

When a validator downloads a snapshot, it can take all live account states, hash to prime, then
add it to the 'deleted' accumulator state and it should end at the 'add' accumulator state.
That will verify that the snapshot account states are correct.

Then, while a validator is processing transactions to catch up to the cluster from the snapshot, use
incoming vote transactions and the commitment calculator to confirm that the cluster is indeed
building on the snapshotted bank hash. Once a threshold commitment level is reached, accept the
snapshot as valid and start voting.
