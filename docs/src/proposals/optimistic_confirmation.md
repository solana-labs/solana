# Optimistic Confirmation

## Primitives

`vote(X, S)` - Votes will be augmented with a "reference" slot, `X` 
which is the **last** ancestor at which this validator voted on this fork
with a proof of switching. Thus as long as a validator keeps voting on the 
same fork, `X` will remain unchanged.All votes will then be of the form
`vote(X, S)`, where `S` is the sorted list of slots `(s, s.lockout)`
being voted for.

Given a vote `vote(X, S)`, let `S.last == vote.last` be the last slot in `S`.

* `Claim`: if a validator submits `vote(X, S)`, the same validator
should not have voted on a slot `s'` where  `X < s' < S.last` and `s'`
is not a descendant of `X` and an ancestor of `S.last`. 

* `Proof`: First note that the vote for `S` must have come after the vote
for `S'` (due to lockouts, the higher vote must have after). Thus, the sequence 
of votes must have been: `X ... S' ... S`. This means after the second vote the
validator must have switched back to the original fork at some `X' > X`. This 
means the vote for `S` should have used `X'` as the "reference" point, so the 
vote should have been of the form `vote(X', S)`, not `vote(X, S)`
(Recall the vote should contain the `latest` switch).

To enforce this, we define the "Optimistic Slashing" slashing conditions. Given
any two distinct votes `vote(X, S)`and `vote(X', S')` by the same validator,
the votes must satisfy:

* `X <= S.last`, `X' <= S'.last`
* All `s` in `S` are ancestors/descendants of one another, 
all `s'` in `S'` areancsestors/descendants of one another,
* 
* `X == X'` implies `S` is parent of `S'` or `S'` is a parent of `S`
* `X' > X` implies `X' > S.last` and `S'.last > S.last` 
and for all `s` in `S`, `s + lockout(s) < X'`
* `X > X'` implies `X > S'.last` and `S.last > S'.last` 
and for all `s` in `S'`, `s + lockout(s) < X`

(The last two rules imply the ranges cannot overlap):
Otherwise the validator is slashed. 

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
    * b. `s` is not a descendant of `validator_vote.last`.
    * c. `s + s.lockout() >= old_vote.last` (implies valdator is still locked
    out on slot `s` at slot `old_vote.last`).

Switching forks without a valid switching proof is slashable.

## Definitions:

Optimistic Confirmation - A block `B` is then said to have achieved
"optimistic confirmation" if `>2/3` of stake have voted with votes `v`
where `Range(v)` for each such `v` includes `B.slot`.

Finalized - A block `B` is said to be finalized if at least one 
correct validator has rooted `B` or a descendant of `B`.

Reverted - A block `B` is said to be reverted if another block `B'` that
is not a parent or descendant of `B` was finalized.

## Guarantees:

A block `B` that has reached optimistic confirmation will not be reverted
unless at least one validator is slashed.

## Proof:
Assume for the sake of contradiction, a block `B` has achieved 
`optimistic confirmation` at some slot `B + n` for some `n`, and:

* Another block `B'` that is not a parent or descendant of `B`
was finalized.
* No validators violated any slashing conditions.

By the definition of `optimistic confirmation`, this means `> 2/3` of validators
have each shown some vote `v` of the form `Vote(X, S)` where `X <= B <= v.last`.
Call this set of votes `Optimistic Votes`.

In order for `B'` to have been rooted, there must have been `> 2/3` stake that 
voted on `B'` or a descendant of `B'`. 

Together, this means `> 1/3` of the staked validators:

* Rooted `B'` or a descendant of `B'`
* Also submitted a vote `v` of the form `Vote(X, S)` where `X <= B <= v.last`.

Let the `Delinquent` set be the set of validators that meet the above
criteria. Let `Delinquent Optimistic Votes`, be the subset of `Optimistic Votes`
containing the **latest** votes from `Optimistic Votes` made by each of these
delinquent validators.

###Lemma 1: 
`Claim:` Given a vote `Vote(X, S)` made by a validator `V` in the
`Delinquent` set, and `S` contains a vote for a slot `s` for which:

* `s + s.lockout > B`, 
* `s` is not an ancestor or descendant of `B`,

then `X > B`.

`Proof`: Assume for the sake of contradiction a delinquent validator `V` made
such a vote `Vote(X, S)` where `S` contains a vote for a slot `s` not an 
ancestor or descendant of `B`, where `s + s.lockout > B`, but `X <= B`.

Let `Vote(X', S')` be the vote in `Delinquent Optimistic Votes`
made by validator `V`. By definition of that set (all votes optimistically 
confirmed `B`), `X' <= B <= S'.last`.

This implies that because its' assumed above `X <= B`, then `X <= S'.last`,
so by the slashing rules, either `X == X'` or `X < X'` (otherwise would
overlap the range `(X', S'.last)`).

`Case X == X'`:

Consider `s`. We know `s != X` because it is assumed `s` is not an ancestor
or descendant of `B`, and `X` is an ancestor of `B`. Because `S'.last` is a
descendant of `B`, this means `s` is also not an ancestor or descendant of 
`S'.last`. Then because `S.last` is descended from `s`, then `S'.last` cannot
be an ancestor or descendant of `S.last` either. This implies `X != X'` by the
"Optimistic Slashing" rules.

`Case X < X'`:

Intuitively, this implies that `Vote(X, S)` was made "before" `Vote(X', S')`.

From the assumption above, `s + s.lockout > B > X'`. Because `s` is not an 
ancestor of `X'`, lockouts would have been violated when this validator
fiirst attempted to submit a switching vote to `X'` with some vote of the
form `Vote(X', S'')`. 

Since none of these cases are valid, the assumption must have been invalid,
and the claim is proven.

###Lemma 2: 
Recall `B'` was the block finalized on a different fork than 
"optimistically" confirmed" block `B`.

`Claim`: For any vote `Vote(X, S)` in the `Delinquent Optimistic Votes` set,
it must be true that `B' > X`

`Proof`: Let `Vote(X, S)` be a vote in the `Delinquent Optimistic Votes` set,
where the for the "optimistcally confirmed" block `B`, `X <= B <= S.last`.

Because `X` is a parent of `B`, and `B'` is not a parent or ancestor of `B`,
then:

* `B' != X`
* `B'` is not a parent of `X`

Now consider if `B'` < `X`:

`Case B' < X`: We wll show this is a violation of lockouts. 
From above, we know `B'` is not a parent of `X`. Then because `B'` was rooted,
and `B'` is not a parent of `X`, then the validator should not have been able
to vote on the higher slot `X` that does not descend from `B'`.

### Proof of Safety:
We now aim to show at least one of the validators in the `Delinquent` set
violated a slashing rule.

By definition, in order to root `B'`, each valdator `V` in `Delinquent`
must have each made some "switching vote" of the form `Vote(X_v, S_v)` where:

* `S_v.last > B'`
* `S_v.last` is a descendant of `B'`, so it can't be a descendant of `B`
* Because `S_v.last` is not a descendant of `B`, then `X_v` cannot be a
descendant or ancestor of `B`.

Recall also this delinquent validator `V` must also have made some vote
`Vote(X, S)` in the `Delinquent Optimistic Votes` set. By definition of that
set (all votes optimistically confirmed `B`), we know `S.last >= B >= X`

By `Lemma 2` we know `B' > X`, and from above `S_v.last > B'`, so then
`S_v.last > X`. Because `X_v != X` (cannot be a descendant or ancestor of 
`B` from above), then by the slashing rules then, we know `X_v > S.last`.
From above, `S.last >= B >= X` so for all such "switching votes", `X_v > B`.

Ordering all these "switching votes" in time, let `V` to be the validator
that first submitted such a "swtching vote" `Vote(X', S')`, where from the
properties derived from above, `X' > B`

Now let `Vote(X, S)` be the vote in `Delinquent Optimistic Votes` made by
validator `V`. 

Given `Vote(X, S)` and the the "Optimistic Slashing" rules, because
`X' > B > X`, then `X' > S.last`.

In order to perform such a "switching vote" to `X'`, a switching proof
`SP(Vote(X, S), Vote(X', S')` must show `> 1/3` of stake being locked
out at this validator's latest vote, `S.last`. Combine this `>1/3` with the
fact that the set of validators in `Delinquent` consists of `> 2/3` of the
stake, implies at least one validator `W` in `Delinquent` must have submitted
a vote (recall the definition of a switching proof), `Vote(X_w, S_w)`
that was included in the switching proof for `X'`, where `S_w` contains a slot
`s` such that:

* `s` is not a common ancestor of `S.last` and `X'`
* `s` is not a descendant of `S.last`.
* `s' + s'.lockout > S.last`

Because `B` is an ancestor of `S.last`, it is also true then:
* `s` is not a common ancestor of `B` and `X'`
* `s' + s'.lockout > B`

which was included in `V`'s switching proof.

Now because `W` is also a member of `Delinquent`, then by the `Lemma 1` above, 
given a vote by `W`, `Vote(X_w, S_w)`, where `S_w` contains a vote for a slot 
`s` where `s + s.lockout > B`, and `s` is not an ancestor of `B`, then
`X_w > B`.

Because validator `V` included vote `Vote(X_w, S_w)` in its proof of switching
for slot `X'`, then his implies validator `V'` submitted vote `Vote(X_w, S_w)`
**before** validator `V` submitted its switching vote for slot `X'`,
`Vote(X', S')`.

But this is a contradiction because we chose `Vote(X', S')` to be the first vote
made by any validator in the `Delinquent` set where `X' > B` and `X'` is not a 
descendant of `B`.