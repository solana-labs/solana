---
title: Block Ancestors
---

There is no easy way to verify the parents of a given block just based on the
shreds in the block. Having this information available in a block would be
useful in scenarios where we want to reason about the structure of the chain
without having to repair and replay all the ancestors. Such scenarios usually include
collecting vote hashes from the cluster, and inferring cluster state like the weight
of different forks based solely on those vote hashes, without needing to perform
full repair/replay.

## Introducing ancestor proofs to a block

Leaders will now include the last `N` ancestors proofs in a sorted list in
the first shreds of their block `B`.

Each item `A_i` in this ancestor proof list represents a block that is an ancestor
of `B`. Each proof `A_i` proves that the block represented by `A_{i-1}` is its direct
parent.

The proof includes the following information about the ancestor block it represents:

1) The slot `S_i`
2) The block hash `blockhash_i` (i.e. the hash of the last tick in the block).
3) The bank hash `bankhash_i`. Recall the bank hash looks like:
```
hash(&[
    parent_bank_hash,
    accounts_delta_hash,
    &signature_count_buf,
    last_blockhash,
]
```
4) A merkle proof showing that `blockhash_i` is indeed the correct blockhash for `bankhash_i`.
From 3) we can see that this implies we need the `parent_bank_hash`, `accounts_delta_hash`, and`signature_count_buf`. Luckily, the `parent_bank_hash` should be available in `A_{i-1}`, for all
but the last ancestor proof, so we can reuse that.

Given the components of the proof, we now have the ability to for any block `A`
for slot `S` that has an ancestor proof (recall a malicious leader may generate multiple
blocks for a slot `S`):

1) Tell whether we have a version of `S` that matches block `A`. This is because from the
ancestor proof from `A`, we can find the expected blockhash. We can then check the last shred in
our version of `S` for the hash of the last tick. If that checks out, we can then verify the hashes of all the entries in the shred are valid.

2) Must be on the same fork as any other block `A'` for some slot `S'` that was also included in the
proof list.

## Removing EpochSlots from gossip

EpochSlots takes a lot of bandwidth in gossip, and we would ultimately like to remove this
burden on the network. Removing EpochSlots has a few fallouts described in the following
sections.

### Propagation Check

TBD

### Finding a repair peer

EpochSlots is currently used to find validators from which to repair a slot `S`

#### Proposed Solution:
We can potentially use people's votes to determine who has which blocks. This might not be as effective for minor forks that people aren't voting on, or if votes are sparse in general. In such
cases it could be more effective to calculate using turbine who is most likely to have the missing
shreds we need, and stake-weight sample potential repair targets.

### Duplicate Slots Protocol

Currently validators run a `AncestorHashesRepairService` that relies on EpochSlots to determine when
a validator has potentially the wrong version of a dead slot. See https://github.com/solana-labs/solana/blob/master/book/src/proposals/handle-duplicate-block.md#the-repair-problem for details.

Without EpochSlots, there needs to be a way to detect and repair a duplicate slot `S` for which the
validator:

1) Has not seen duplicate threshold of the cluster vote on any *single* version of the duplicate
slot `S`.
2) However, a sufficient portion of the cluster has voted on some set of descendants `D` of some
version `X` of the duplicate slot, such that `X` is actually duplicate confirmed.
3) The validator has the wrong version of `S` and thus cannot replay the descendants.

The current validator cannot handle this situation without EpochSlots because it has no way of
knowing the set of descendants in `D` are on the same fork as `X` without replaying `X` and all its
descendants. However, due to 3) the validator does not have `X`, and will never replay `X` without
first dumping its incorrect version of the slot. Thus, this validator will never see duplicate
confirmation on the duplicate slot and will fail to dump its version and repair the correct version
of `S`.

#### Proposed Solution:

1) Introduce a new repair protocol to fetch the ancestor proofs of any given block `B`. The
protocol looks like `GetAncestorProofs(Hash)`, where the `Hash` is the bank hash of `B`. Responders
reply with the ancestor proofs in the block for `B`.

2) For all votes we see in gossip greater than the root, validators track the `(slot, bank hash)`
of those votes in a tree very similar to the `HeaviestForkChoice` tree we use for consensus. The parent of each node in the tree is the parent of the block represented by the `(slot, bank hash)`
in that node.

3) Nodes in the tree are weighted by the number of votes we've seen for that node or any of
its descendants.

4) Nodes in the tree also need to track when they get duplicate confirmation. This means there
needs to be a historical record for all the validators that have *ever* voted on a particular branch
in the tree. This is not currently supported by `HeaviestForkChoice` as it currently implements
LMD ghost, where it only tracks the *latest* vote made by each validator.

4) There are two groups of trees we track, very similar to the structure managed in
`repair_weight.rs` used for repair.
    a) One is the main "trunk" where the root of the tree is the validator's current root
    b) A set of "orphans". Each tree in the "orphans" represents a branch for which we don't
    know the connection to the main "trunk" in a). In other words, if a tree is in the "orphan"
    set, the root of the tree is a block for which we don't know the parent

5) On every iteration of the protocol, we try to find the heaviest orphans and run the repair
procedure from 1) to try and find its parents. Eventually, this should lead to connecting
the orphan back to the main "trunk", and we can merge the trees, or we find the orphan tree is not descended from the validator's root. In both cases the orphan is removed from the orphan set.


