# Deterministic Transaction Fees

Transactions currently include a fee field that indicates the maximum fee field
a slot leader is permitted to charge to process a transaction. The cluster, on
the other hand, agrees on a minimum fee. If the network is congested, the slot
leader may prioritize the transactions offering higher fees. That means the
client won't know how much was collected until the transaction is confirmed by
the cluster and the remaining balance is checked. It smells of exactly what we
dislike about Ethereum's "gas", non-determinism.

## Implementation Status

This design is not yet implemented, but is written as though it has been.  Once
implemented, delete this comment.

### Congestion-driven fees

Each validator submits network congestion data to its voting account.  The
cluster then uses a stake-weighted average from all voting accounts to
calculate fee parameters. In the first implementation of this design, `tps` and
`tps_capacity` are the only statistics uploaded. They are uploaded once per
epoch.

### Calculating fees

The client uses the JSON RPC API to query the cluster for the current fee
parameters.  Those parameters are tagged with a blockhash and remain valid
until that blockhash is old enough to be rejected by the slot leader.

Before sending a transaction to the cluster, a client may submit the
transaction and fee account data to an SDK module called the *fee calculator*.
So long as the client's SDK version matches the slot leader's version, the
client is assured that its account will be changed exactly the same number of
lamports as returned by the fee calculator.

### Fee Parameters

In the first implementation of this design, the only fee parameter is
`lamports_per_signature`. The more signatures the cluster needs to verify, the
higher the fee. The exact number of lamports is determined by the number of
transactions per second the cluster processed in the previous epoch versus the
cluster's capacity.

Future parameters might include:

* `lamports_per_pubkey` - cost to load an account
* `lamports_per_slot_distance` - higher cost to load very old accounts
* `lamports_per_byte` - cost per size of account loaded
* `lamports_per_bpf_instruction` - cost to run a program
