# Optimistic Confirmation

## Primitives

`vote(X, S)` - Votes will be augmented with an "latest switch", `X` 
which is the **latest** ancestor at which this validator voted on this fork
with a proof of switching. All votes will then be of the form `vote(X, S)`, where 
`S` is the sorted list of slots being voted for. 

Given a vote `vote(X, S)`, let `S.last == vote.last` be the last slot in `S`.

* Lemma 1: if a validator submits `vote(X, S)`, the same validator
should not have voted on a slot `s'` where  `X < s' < S.last` and `s'`
is not a descendant of `X` or an ancestor of `S.last`. 

* Proof: First note that the vote for `S` must have come after the vote
for `S'` (due to lockouts, the higher vote must have after). Thus, the sequence 
of votes must have been: `X ... S' ... S`. This means after the second vote the
validator must have switched back to the original fork at some `X' > X`. This 
means the vote for `S` should have been of he form `vote(X', S)`, not `vote(X, S)`
(Recall the vote should contain the `latest` switch).

To enforce this, we can then say any votes `vote(X, S)` and `vote(X', S')` 
where either:

* `X' > X` and `S'.last <= S.last` 
* `X' < X` and `S'.last >= S.last`

are slashable.

`Range(vote)` - Given a vote `v = vote(X, S)`, define `Range(v)` to be the range
 of slots `[X, S.last]`.

`SP(old_vote, new_vote)` - This is the "Switching Proof" for `old_vote`, the
validator's latest vote. Such a proof is necessary for a validator to vote 
on any slot `new_vote.last` that is not a descendant of `old_vote.last`. Note 
switching must still respect lockouts.

A switching proof shows that `> 1/3` of the network is locked out at slot
`old_vote.last`.

 The proof is a list of elements `(validator_id, validator_vote(X, S))`, where:

1. The sum of the stakes of all the validator id's `> 1/3`

2. For each `(validator_id, validator_vote(X, S))`, there exists some slot `s`
in `S` where: 
    * a.`s` is not a common ancestor of both `validator_vote.last` and
    `old_vote.last` and `new_vote.last`.
    * b. `s + s.lockout() >= old_vote.last` (implies valdator is still locked
    out on slot `s` at slot `old_vote.last`).

Switching forks without a valid switching proof is slashable.

## Definitions:

Optimistic Confirmation - A block `B` is then said to have achieved
"optimistic confirmation" if `>2/3` of stake has voted with some votes `v`
where `v.range` includes `B.slot`.

Finalized - A block `B` is said to be finalized if `>1/3` of the stake
has rooted `B` or a descedant of `B`.

Reverted - A block `B` is said to be reverted if another block `B'` that
is not a parent or descendant of `B` was finalized.

## Guarantees:

A block `B` that has reached optimistic confirmation will not be reverted
unless at least one validator is slashed.

## Proof:
AFSOC a block `B` has achieved `optimistic confirmation` at some slot `B + n`
for some `n`, and:
* Another block `B'` that is not a parent or descendant of `B`
was finalized.
* No validators violated any slashing conditions.

By the definition of `optimistic confirmation`, this means `> 2/3` of validators
have each shown some vote `v` of the form `Vote(X, S)` where `X <= B <= v.last`.
Call this set of votes `Optimistic Votes`

In order for `B'` to have been rooted, there must have been `> 1/3` stake that 
voted on `B'` or a descendant of `B'`. 

Together, this means `> 0` of the stake:
* Voted on `B'` or a descendant of `B'`
* Also submitted a vote `v` of the form `Vote(X, S)` where `X <= B <= v.last`.

Call this set of `> 0` of the stake `Delinquent`. Let `Optimistic Optimitic Votes`,
be the subset of `Optimistic Votes` whch includes the **latest** vote that belongs
to each of these delinquents.

We now aim to show at least one of the validators in `Delinquent` violated
a slashing rule.

Let `latest_vote = Vote(X_e, S_e)` be a vote in `Delinquent Optimistic Votes`
with the "earliest" latest vote, so `S_e.last <= v.last` for all `v` in 
`Delinquent Optimistic Votes`. Let `V_e` be the "least" validator
that submitted that vote. Note here because `X_e` is a parent of `B`, we know:

* `B' != X_e`
* `B'` is not a parent of `X`

because if any of the above were true, `B'` would be a parent off `B`.

Now consider the two remaning cases, either `B'` < `X_e` or `B' > X_e`.

`Case B' < X_e`: This is a violation of lockouts. 
From above, we know `B'` is not a parent of `X_e`. Then because `B'` was rooted,
and `B'` is not a parent of `X_e`, then the validator should not have been able
to vote on the higher slot `X_e` that does not descend from `B'`.

`Case B' > X_e`: By definition of `Delinquent Optimistic Votes`, we know 
`Range(latest_vote) = Range(Vote(X_e, S_e))` includes `B`, so `B < S_e.last`.

By the slashing rules established n `Lemma 1`, because the validator submitted
`latest_vote`, they could not have voted on a slot `s'` where:

* `X_e < s' < S_e.last`
* `s'` is not a descendant of `X_e` or an ancestor of `S_e`.

Because as noted above `B < S_e.last`, this means, no vote for slot `s` 
can exist where:

* `X_e < s' < B`
* `s'` is not a descendant of `X_e` or an ancestor of `S`.

Combined with the above assumptions `B' > X_e`, and because we know 
`B'` not an ancestor of `S`, this means `B' > B`. 

This means the earliest vote this "earliest" validator `V_e` could have 
submitted switching to the fork containing `B'` occurred at some slot `X'`
after `B`. This implies a valid switching proof was presented for `X'`.
Such a proof must show `> 1/3` of stake being locked out at this validator's
latest vote, `S_e.last`. Because the set of validators in `Delinquent` consists
of `2/3` of the stake, this means at least one validator `V` in `Delinquent`
must have submitted a vote that is "locked out" at `S_e.last`. 

More formally, this means (recall the definition of a switching proof),
the validator `V` has submitted some vote `Vote(X_v, S_v)` where `S_v`
contains a slot `s` such that:

* `s` is not a common ancestor of `S.last` and `X'`
* `s' + s'.lockout > S_e.last`

which was included in `V_e`'s switching proof.

Now because `V` is a member of `Delinquent`, it must also have made some vote
of the form `Vote(X_v', S_v')` where `S_v'.last > B` in the set
`Delinquent Optimistic Votes`. Thus, any lockouts on slots < `X_v'` that were
not ancestors of `X_v'`must have expired by `X_v` (Otherwise the vote on `X_v'`
could not have happened). This means `s' > X_v'`.

By `Lemma 1`, this validator also could not have voted on another fork in the range
`[X_v', S_v']`, so then `s' > S_v'.last`. 

Now recall we chose `S_e` to be the "earliest" in the `Delinquent Optimistic Votes`,
`S_e.last >= S_v'.last`, and thus `s' >= S_e.last`.

But this is a contradiction because in order for `s'` to be used in the switching
proof,`s' + s'.lockout > S_e.last`.

However, we assumed this valdator was the earliest such validator, so there cannot 
exist another in this set.










