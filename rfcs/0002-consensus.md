# Consensus

The goal of this RFC is to define the consensus algorithm used in Solana.  This proposal covers a Proof of Stake (PoS) algorithm that leverages Proof of History (PoH).  PoH is a permissionless clock for blockchain that is available before consensus.  This PoS approach leverages PoH to make strong assumptions about time among partitions.


## Basic Design Idea

Nodes on the network can be "up" or "down".  A node indicates it is up either by voting as a validator or by generating a PoH stream as the designated leader.  Consensus is reached when a supermajority + 1 of the staked nodes have voted on the state of the network at a particular PoH tick count.

Nodes take turns being leader and generating the PoH that encodes state changes.  The network can tolerate loss of connection to any leader by synthesizing what the leader ***would have generated*** had it been connected but not ingesting any state changes.  The complexity of forks is thereby limited to a "there/not-there" skip list of branches that may arise on leader rotation periods boundaries.


## Message Flow

1. Transactions are ingested at the current leader.
2. Leader filters for valid transactions.
3. Leader executes valid transactions on its state.
4. Leader packages transactions into entries based off the longest observed PoH branch.
5. Leader transmits the entries to validator nodes (in signed blobs)
    a. The set of supermajority + `M` by stake weight of nodes is rotated in round robin fashion.
    b. The PoH stream includes ticks; empty entries that indicate liveness of the leader and the passage of time on the network.
    c. A leader's stream begins with the tick entries necessary complete the PoH back to that node's most recently observed prior leader period.
6. Validators retransmit entries to peers in their set and to further downstream nodes.
7. Validators validate the transactions and execute them on their state.
8. Validators compute the hash of the state.
9. At specific times, i.e. specific PoH tick counts, validators transmit votes to the leader.
    a. Votes are signatures of the hash of the computed state at that PoH tick count
10. Leader executes the votes as any other transaction and broadcasts them to the network
    a. The leader votes at that same height once a majority of stake is represented on the PoH stream *(open question: do leaders vote?)*
11. Validators observe their votes and all the votes from the network.
12. Validators vote on the longest chain of periods that contains their vote.

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

Creating the stake has a warmup period of TBD.  Unstaking requires the node to miss a certain number of validation voting rounds.

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
    PoH tick count,
    PoH hash,
    stake public key,
    ...
    signature of the message with the stake keypair
))
```

When the `Slash` vote is processed, validators should lookup `PoH hash` at `PoH count` and compare it with the message.  If they do not match, the stake at `stake public key` should be set to `0`.

## Leader Slashing

The goal is to discourage leaders from generating multiple PoH streams.  When this occurs, the network adopts ticks for that leader's period.  Leaders can be slashed for generating multiple conflicting PoH streams during their period.

## Validation Vote Contract

The goal of this contract is to simulate economic cost of mining on a shorter branch.

1. With my signature I am certifying that I computed `state hash` at `PoH count tick count` and `PoH hash`.
2. I will not vote on a branch that doesn't contain this message for at least `N` counts, or until `PoH tick count` + `N` is reached by the PoH stream (lockout period).
3. I will not vote for any other branch below `PoH count`.
    a. if there are other votes not present in this PoH history the validator may need to `cancel` them before creating this vote.
4. Each vote on a branch increases the lockout for all prior votes on that branch according to a network-specified function.
