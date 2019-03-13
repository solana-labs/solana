# Deterministic Transaction Fees

Transactions currently include a fee field that indicates the maximum fee field
a slot leader is permitted to charge to process a transaction. The cluster, on
the other hand, agrees on a minimum fee. If the network is congested, the slot
leader may prioritize the transactions offering higher fees. That means the
client won't know how much was collected until the transaction is confirmed by
the cluster and the remaining balance is checked. It smells of exactly we
dislike about Ethereum's "gas", non-determinism.

## Implementation Status

This design is not yet implemented, but is written as though it has been.  Once
implemented, delete this comment.

### The Fees Program

The Solana cluster has an on-chain program called Fees with just one global
account called the *fees account*. The account holds a set of fee parameters
that the cluster will use to calculate transaction fees in the following epoch.
Any slot leader may submit congestion statistics to the fees account and the
Fees program uses that data to automatically adjust the fees to maximize
resource usage.  Each validator checks congestion statistics and rejects blocks
that include congestion data inconsistent with its own observations.

The client uses the JSON RPC API to query the cluster for the current fee
parameters.  Those parameters are tagged with a blockhash and remain valid
until that blockhash is old enough to be rejected by the slot leader.

### Calculating fees

The fees account contains a variety of fee parameters. Before sending a
transaction to the cluster, a client may submit the transaction and fee account
data to an SDK module called the *fee calculator*. So long as the client's SDK
version matches the slot leader's version, the client is assured that its
account will be changed exactly the same number of lamports as returned by the
fee calculator.

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
