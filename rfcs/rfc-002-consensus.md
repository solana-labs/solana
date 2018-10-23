# Consensus

The goal of this RFC is to define the consensus algorithm used in Solana.  This proposal covers a Proof of Stake (PoS) algorithm that leverages Proof of History (PoH).  PoH is a permissionless clock for blockchain that is available before consensus.  This PoS approach leverages PoH to make strong assumptions about time among partitions.


## Version

version 0.3

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

## Leader Seed Generation

Leader selection is decided via a random seed.  The process is as follows:

1. Periodically at a specific `PoH tick count` select the first vote signatures that create a supermajority from the previous voting round.
2. Append them together.
3. Hash the string for `N` counts via a similar process as PoH itself.
4. The resulting hash is the random seed for `M` counts, `M` leader periods, where M > N

## Leader Rotation

1. The leader is chosen via a random seed generated from stake weights and votes (the leader schedule)
2. The leader is rotated every `T` PoH ticks (leader period), accoding to the leader schedule
3. The schedule is applicable for `M` voting rounds

Leader's transmit for a count of `T` PoH ticks.  When `T` is reached all the validators should switch to the next scheduled leader.  To schedule leaders, the supermajority + `M` nodes are shuffled using the above calculated random seed.

All `T` ticks must be observed from the current leader for that part of PoH to be accepted by the network.  If `T` ticks (and any intervening transactions) are not observed, the network optimistically fills in the `T` ticks, and continues with PoH from the next leader.

## Partitions, Forks

Forks can arise at PoH tick counts that correspond to leader rotations, because leader nodes may or may not have observed the previous leader's data.  These empty ticks are generated by all nodes in the network at a network-specified rate for hashes/per/tick `Z`.

There are only two possible versions of the PoH during a voting period: PoH with `T` ticks and entries generated by the current leader, or PoH with just ticks.  The "just ticks" version of the PoH can be thought of as a virtual ledger, one that all nodes in the network can derive from the last tick in the previous period.

Validators can ignore forks at other points (e.g. from the wrong leader), or slash the leader responsible for the fork.

Validators vote on the longest chain that contains their previous vote, or a longer chain if the lockout on their previous vote has expired.


#### Validator's View

##### Time Progression
The diagram below represents a validator's view of the PoH stream with possible forks over time.  L1, L2, etc. are leader periods, and `E`s represent entries from that leader during that leader's period.  The 'x's represent ticks only, and time flows downwards in the diagram.


```
time  +----+                                          validator action
|     | L1 |                 E(L1)
|     |----|            /             \                vote(E(L2))
|     | L2 |        E(L2)               x
|     |----|        /    \           /     \           vote(E(L2))
|     | L3 |    E(L3)      x       E(L3)'    x
|     |----|    /  \     /   \     /  \     /  \       slash(L3)
|     | L4 |  x     x  E(L4)  x   x    x   x    x
V     |----|  |     |  |      |   |    |   |    |      vote(E(L4))
V     | L5 |  xx   xx  xx   E(L5) xx   xx  xx   xx
V     +----+                                           hang on to E(L4) and E(L5) for more...

```

Note that an `E` appearing on 2 branches at the same period is a slashable condition, so a validator observing `E(L3)` and `E(L3)'` can slash L3 and safely choose `x` for that period.  Once a validator observes a supermajority vote on any branch, other branches can be discarded below that tick count.  For any period, validators need only consider a single "has entries" chain or a "ticks only" chain.

##### Time Division

It's useful to consider leader rotation over PoH tick count as time division of the job of encoding state for the network.  The following table presents the above tree of forks as a time-divided ledger.

leader period |  L1 | L2 | L3 | L4 | L5
-------|----|----|----|----|----
data      |  E(L1)| E(L2) | E(L3) | E(L4)  | E(L5)
ticks to prev  | | | | x | xx

Note that only data from leader L3 will be accepted during leader period L3.  Data from L3 may include "catchup" ticks back to a period other than L2 if L3 did not observe L2's data.  L4 and L5's transmissions include the "ticks to prev" PoH entries.

This arrangement of the network data streams permits nodes to save exactly this to the ledger for replay, restart, and checkpoints.

#### Leader's View

When a new leader begins a period, it must first transmit any PoH (ticks) required to link the new period with the most recently observed and voted period.


## Examples

### Small Partition
1. Network partition M occurs for 10% of the nodes
2. The larger partition K, with 90% of the stake weight continues to operate as normal
3. M cycles through the ranks until one of them is leader, generating ticks for periods where the leader is in K.
4. M validators observe 10% of the vote pool, finality is not reached.
5. M and K re-connect.
6. M validators cancel their votes on M, which has not reached finality, and re-cast on K (after their vote lockout on M).

### Leader Timeout
1. Next rank leader node V observes a timeout from current leader A, fills in A's period with virtual ticks and starts sending out entries.
2. Nodes observing both streams keep track of the forks, waiting for:
    a. their vote on leader A to expire in order to be able to vote on B
    b. a supermajority on A's period
3. If a occurs, leader B's period is filled with ticks, if b occurs, A's period is filled with ticks
4. Partition is resolved just like in the [Small Partition](#small-parition)


## Network Variables

`M` - number of nodes outside the supermajority to whom leaders broadcast their PoH for validation

`N` - number of voting rounds for which a leader schedule is considered before a new leader schedule is used

`T` - number of PoH ticks per leader period (also voting period)

`Z` - number of hashes per PoH tick
