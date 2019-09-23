# Deterministic Transaction Fees

Transactions currently include a fee field that indicates the maximum fee field a slot leader is permitted to charge to process a transaction. The cluster, on the other hand, agrees on a minimum fee. If the network is congested, the slot leader may prioritize the transactions offering higher fees. That means the client won't know how much was collected until the transaction is confirmed by the cluster and the remaining balance is checked. It smells of exactly what we dislike about Ethereum's "gas", non-determinism.

## Congestion-driven fees

Each validator uses _signatures per slot_ \(SPS\) to estimate network congestion and _SPS target_ to estimate the desired processing capacity of the cluster. The validator learns the SPS target from the genesis block, whereas it calculates SPS from recently processed transactions. The genesis block also defines a target `lamports_per_signature`, which is the fee to charge per signature when the cluster is operating at _SPS target_.

## Calculating fees

The client uses the JSON RPC API to query the cluster for the current fee parameters. Those parameters are tagged with a blockhash and remain valid until that blockhash is old enough to be rejected by the slot leader.

Before sending a transaction to the cluster, a client may submit the transaction and fee account data to an SDK module called the _fee calculator_. So long as the client's SDK version matches the slot leader's version, the client is assured that its account will be changed exactly the same number of lamports as returned by the fee calculator.

## Fee Parameters

In the first implementation of this design, the only fee parameter is `lamports_per_signature`. The more signatures the cluster needs to verify, the higher the fee. The exact number of lamports is determined by the ratio of SPS to the SPS target. At the end of each slot, the cluster lowers `lamports_per_signature` when SPS is below the target and raises it when above the target. The minimum value for `lamports_per_signature` is 50% of the target `lamports_per_signature` and the maximum value is 10x the target \`lamports\_per\_signature'

Future parameters might include:

* `lamports_per_pubkey` - cost to load an account
* `lamports_per_slot_distance` - higher cost to load very old accounts
* `lamports_per_byte` - cost per size of account loaded
* `lamports_per_bpf_instruction` - cost to run a program

## Attacks

### Hijacking the SPS Target

A group of validators can centralize the cluster if they can convince it to raise the SPS Target above a point where the rest of the validators can keep up. Raising the target will cause fees to drop, presumably creating more demand and therefore higher TPS. If the validator doesn't have hardware that can process that many transactions that fast, its confirmation votes will eventually get so long that the cluster will be forced to boot it.

