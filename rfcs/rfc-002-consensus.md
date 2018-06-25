# Consensus

VERY WIP

The goal of this RFC is to define the consensus algorithm used in solana.  This proposal covers a Proof of Stake algorithm that leverages Proof of History.  PoH is a permissionless clock for blockchain that is available before consensus.  This PoS approach leverages PoH to make strong assumptions about time between partitions.

## Version

version 0.1

## Message Flow

1. Transactions are ingested at the leader.
2. Leader filters for valid transactions
3. Leader executes valid transactions on its state
4. Leader packages transactions into blobs
5. Leader transmits blobs to validator nodes.
    a. The set of supermajority + `M` by stake weight of nodes is rotated in round robin fashion.
6. Validators retransmit blobs to peers in their set and to further downstream nodes.
7. Validators validate the transactions and execute them on their state.
8. Validators compute the hash of the state.
9. Validators transmit votes to the leader.
    a. Votes are signatures of the hash of the computed state.
10. Leader executes the votes as any other transaction and broadcasts them out to the network
11. Validators observe their votes, and all the votes from the network.
12. Validators continue voting if the supermajority of stake is observed in the vote for the same hash.

Supermajority is defined as `2/3rds + 1` vote of the PoS stakes.

## Staking

Validators `stake` some of their spendable sol into a staking account.  The stakes are not spendable and can only be used for voting.

```
CreateStake(
    PoH count,
    PoH hash,
    source public key,
    amount,
    destination public key,
    proof of ownership of destination public key,
    signature of the message with the source keypair
)
```

Creating the stake has a warmup period of TBD.  Unstaking requires the node to miss a certain amount of validation votes.

## Validation Votes

```
Validate(
    PoH count,
    PoH hash,
    stake public key,
    signature of the state,
    signature of the message with the stake keypair
)
```

## Validator Slashing

Validators `stake` some of their spendable sol into a staking account.  The stakes are not spendable and can only be used for voting.

```
Slash(Validate(
    PoH count,
    PoH hash,
    stake public key,
    ...
    signature of the message with the stake keypair
))
```

When the `Slash` vote is processed, validators should lookup `PoH hash` at `PoH count` and compare it with the message.  If they do not match, the stake at `stake public key` should be set to `0`.

## Leader Slashing

TBD.  The goal of this is to discourage leaders from generating multiple PoH streams.

## Validation Vote Contract

The goal of this contract is to simulate economic cost of mining on a shorter branch.

1. With my signature I am certifying that I computed `state hash` at `PoH count` and `PoH hash`.
2. I will not vote on a branch that doesn't contain this message for at least `N` counts, or until `PoH count` + `N` is reached by the PoH stream.
3. I will not vote for any other branch below `PoH count`.
    a. if there are other votes not present in this PoH history the validator may need to `cancel` them before creating this vote.

## Leader Seed Generation

Leader selection is decided via a random seed.  The process is as follows:

1. Periodically at a specific `PoH count` select the first vote signatures that create a supermajority from the previous round.
2. append them together
3. hash the string for `N` counts via a similar process as PoH itself.
4. The resulting hash is the random seed for `M` counts, where M > N

## Leader Ranking and Rotation

Leader's transmit for a count of `T`.  When `T` is reached all the validators should switch to the next ranked leader.  To rank leaders, the supermajority + `M` nodes are shuffled with the using the above calculated random seed.

TBD: define a ranking for critical partitions without a node from supermajority + `M` set.

## Partition selection

Validators should select the first branch to reach finality, or the highest ranking leader.

## Examples

### Small Partition
1. Network partition M occurs for 10% of the nodes
2. The larger partition K, with 90% of the stake weight continues to operate as normal
3. M cycles through the ranks until one of them is leader.
4. M validators observe 10% of the vote pool, finality is not reached
5. M and K re-connect.
6. M validators cancel their votes on K which are below K's `PoH count`

### Leader Timeout
1. Next rank node observes a timeout.
2. Nodes receiving both PoH streams pick the higher rank node.
3. 2, causes a partition, since nodes can only vote for 1 leader.
4. Partition is resolved just like in the [Small Partition](#small-parition)
