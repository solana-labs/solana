# Confidence

The confidence metric aims to give clients a measure of the network 
confirmation levels on a particular block.

# Calculation RPC

Clients can request confidence metrics from a validator for a signature `s` 
through `get_confidence(s: Signature) -> Confidence` over RPC. The 
`Confidence` struct is of type `Vec<u32>`. This returned vector 
represents the confidence metric for the particular block `N` that contains the
signature `s` as of the last block `M` that the validator voted on. 

An entry `s` at index `i` in the `Confidence` vector implies that the validator
observed `s` total stake in the network reaching `i` confirmations on block `N`
as observed in some block `M`. There will be `MAX_LOCKOUT` elements in this 
vector, representing all the possible number of confirmations from 1 and to
`MAX_LOCKOUT`.

# Computation of confidence metric

Builiding this `Confidence` vector leverages the computations already being
performed for building consensus. The `collect_vote_lockouts` function in 
`consensus.rs` builds a HashMap, where each entry is of the form `(b, s)`
where `s` is a `StakeLockout` struct representing the amount of stake and 
lockout on a bank `b`.

This computation is performed on a votable candidate bank `b` as follows.

```
   let output: HashMap<b, StakeLockout> = HashMap::new();
   for vote_account in b.vote_accounts {
       for v in vote_account.vote_stack {
           for a in ancestors(v) {
               f(*output.get_mut(a), vote_account, v);
           }
       }
   }
```

where `f` is some accumulation function that modifies the `StakeLockout` entry
for slot `a` with some data derivable from vote `v` and `vote_account`
(stake, lockout, etc.). Note here that the `ancestors` here only includes 
slots that are present in the current status cache. Signatures for banks earlier
than those present in the status cache would not be queryable anyway, so those
banks are not included in the confidence calculations here.

Now we can naturally augment the above computation to also build a `Confidence`
vector for every bank `b` by:
1) Augmenting `StakeLockout` to include a `confidence` field that represents a
`Confidence` vector
2) Replacing `f` with `f'` such that the above computation also builds this
`Confidence` vector for every bank `b`.

We will proceed with the details of 2) as 1) is trivial.

Before continuing, it is noteworthy that for some validator's vote account `a`,
the number of local confirmations for that validator on slot `s` is 
`v.num_confirmations`, where `v` is the smallest vote in the stack of votes 
`a.votes` such that `v.slot >= s` (i.e. there is no need to look at any 
votes > v as the number of confirmations will be lower).

Now more specifically, we augment the above computation to:

```
   let output: HashMap<b, StakeLockout> = HashMap::new();
   for vote_account in b.vote_accounts {
       // vote stack is sorted from oldest vote to newest vote
       for (v1, v2) in vote_account.vote_stack.windows(2) {
           for a in ancestors(v1).difference(ancestors(v2)) {
               f'(*output.get_mut(a), vote_account, v);
           }
       }
   }
```

where `f'` is defined as:
```
    fn f`(
        some_ancestor: &mut StakeLockout, 
        vote_account: VoteAccount, 
        v: Vote, total_stake: u64
    ){
        f(some_ancestor, vote_account, v);
        *some_ancestor.confidence[v.num_confirmations] += vote_account.stake;
    }
```