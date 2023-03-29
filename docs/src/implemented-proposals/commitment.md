---
title: Commitment
---

The commitment metric aims to give clients a measure of the network confirmation
and stake levels on a particular block. Clients can then use this information to
derive their own [measures of commitment](../cluster/commitments.md).

# Calculation RPC

Clients can request commitment metrics from a validator for a signature `s`
through `get_block_commitment(s: Signature) -> BlockCommitment` over RPC. The
`BlockCommitment` struct contains an array of u64 `[u64, MAX_CONFIRMATIONS]`. This
array represents the commitment metric for the particular block `N` that
contains the signature `s` as of the last block `M` that the validator voted on.

An entry `s` at index `i` in the `BlockCommitment` array implies that the
validator observed `s` total stake in the cluster reaching `i` confirmations on
block `N` as observed in some block `M`. There will be `MAX_CONFIRMATIONS` elements in
this array, representing all the possible number of confirmations from 1 to
`MAX_CONFIRMATIONS`.

# Computation of commitment metric

Building this `BlockCommitment` struct leverages the computations already being
performed for building consensus. The `collect_vote_lockouts` function in
`consensus.rs` builds a HashMap, where each entry is of the form `(b, s)`
where `s` is the amount of stake on a bank `b`.

This computation is performed on a votable candidate bank `b` as follows.

```text
   let output: HashMap<b, Stake> = HashMap::new();
   for vote_account in b.vote_accounts {
       for v in vote_account.vote_stack {
           for a in ancestors(v) {
               f(*output.get_mut(a), vote_account, v);
           }
       }
   }
```

Where `f` is some accumulation function that modifies the `Stake` entry
for slot `a` with some data derivable from vote `v` and `vote_account`
(stake, lockout, etc.). Note here that the `ancestors` here only includes
slots that are present in the current status cache. Signatures for banks earlier
than those present in the status cache would not be queryable anyway, so those
banks are not included in the commitment calculations here.

Now we can naturally augment the above computation to also build a
`BlockCommitment` array for every bank `b` by:

1. Adding a `ForkCommitmentCache` to collect the `BlockCommitment` structs
2. Replacing `f` with `f'` such that the above computation also builds this
   `BlockCommitment` for every bank `b`.

We will proceed with the details of 2) as 1) is trivial.

Before continuing, it is noteworthy that for some validator's vote account `a`,
the number of local confirmations for that validator on slot `s` is
`v.num_confirmations`, where `v` is the smallest vote in the stack of votes
`a.votes` such that `v.slot >= s` (i.e. there is no need to look at any
votes > v as the number of confirmations will be lower).

Now more specifically, we augment the above computation to:

```text
   let output: HashMap<b, Stake> = HashMap::new();
   let fork_commitment_cache = ForkCommitmentCache::default();
   for vote_account in b.vote_accounts {
       // vote stack is sorted from oldest vote to newest vote
       for (v1, v2) in vote_account.vote_stack.windows(2) {
           for a in ancestors(v1).difference(ancestors(v2)) {
               f'(*output.get_mut(a), *fork_commitment_cache.get_mut(a), vote_account, v);
           }
       }
   }
```

where `f'` is defined as:

```text
    fn f`(
        stake: &mut Stake,
        some_ancestor: &mut BlockCommitment,
        vote_account: VoteAccount,
        v: Vote, total_stake: u64
    ){
        f(stake, vote_account, v);
        *some_ancestor.commitment[v.num_confirmations] += vote_account.stake;
    }
```
