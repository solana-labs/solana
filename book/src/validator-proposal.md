# Anatomy of a Validator

## History

When we first started Solana, the goal was to de-risk our TPS claims. We knew
that between optimistic concurrency control and sufficiently long leader slots,
that PoS consensus was not the biggest risk to TPS. It was GPU-based signature
verification, software pipelining and concurrent banking. Thus, the TPU was
born. After topping 100k TPS, we split the team into one group working toward
710k TPS and another to flesh out the validator pipeline. Hence, the TVU was
born. The current architecture is a consequence of incremental development with
that ordering and project priorities. It is not a reflection of what we ever
believed was the most technically elegant cross-section of those technologies.
In the context of leader rotation, the strong distinction between leading and
validating is blurred.

## Difference between validating and leading

The fundamental difference between the pipelines is when the PoH is present. In
a leader, we process transactions, removing bad ones, and then tag the result
with a PoH hash. In the validator, we verify that hash, peel it off, and
process the transactions in exactly the same way. The only difference is that
if a validator sees a bad transaction, it can't simply remove it like the
leader does, because that would cause the PoH hash to change.  Instead, it
rejects the whole block. The other difference between the pipelines is what
happens *after* banking. The leader broadcasts entries to downstream validators
whereas the validator will have already done that in RetransmitStage, which is
a confirmation time optimization.  The validation pipeline, on the other hand,
has one last step. Any time it finishes processing a block, it needs to weigh
any forks it's observing, possibly cast a vote, and if so, reset its PoH hash
to the block hash it just voted on.

## Proposed Design

We unwrap the many abstraction layers and build a single pipeline that can
toggle leader mode on whenever the validator's ID shows up in the leader
schedule.

<img alt="Validator block diagram" src="img/validator-proposal.svg" class="center"/>

## Notable changes

* No threads are shut down to switch out of leader mode. Instead, FetchStage
  should forward transactions to the next leader.
* Hoist FetchStage and BroadcastStage out of TPU
* Blocktree renamed to Blockstore
* BankForks renamed to Banktree
* TPU moves to new socket-free crate called solana-tpu.
* TPU's BankingStage absorbs ReplayStage
* TVU goes away
* New RepairStage absorbs Blob Fetch Stage and repair requests
* JSON RPC Service is optional - used for debugging. It should instead be part
  of a separate `solana-blockstreamer` executable.
* New MulticastStage absorbs retransmit part of RetransmitStage
* MulticastStage downstream of Blockstore

