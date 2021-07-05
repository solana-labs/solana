---
title: Slashing rules
---

Unlike Proof of Work \(PoW\) where off-chain capital expenses are already
deployed at the time of block construction/voting, PoS systems require
capital-at-risk to prevent a logical/optimal strategy of multiple chain voting.
We intend to implement slashing rules which, if broken, result some amount of
the offending validator's deposited stake to be removed from circulation. Given
the ordering properties of the PoH data structure, we believe we can simplify
our slashing rules to the level of a voting lockout time assigned per vote.

I.e. Each vote has an associated lockout time \(PoH duration\) that represents
a duration by any additional vote from that validator must be in a PoH that
contains the original vote, or a portion of that validator's stake is
slashable. This duration time is a function of the initial vote PoH count and
all additional vote PoH counts. It will likely take the form:

```text
Lockouti\(PoHi, PoHj\) = PoHj + K \* exp\(\(PoHj - PoHi\) / K\)
```

Where PoHi is the height of the vote that the lockout is to be applied to and
PoHj is the height of the current vote on the same fork. If the validator
submits a vote on a different PoH fork on any PoHk where k &gt; j &gt; i and
PoHk &lt; Lockout\(PoHi, PoHj\), then a portion of that validator's stake is at
risk of being slashed.

In addition to the functional form lockout described above, early
implementation may be a numerical approximation based on a First In, First Out
\(FIFO\) data structure and the following logic:

- FIFO queue holding 32 votes per active validator
- new votes are pushed on top of queue \(`push_front`\)
- expired votes are popped off top \(`pop_front`\)
- as votes are pushed into the queue, the lockout of each queued vote doubles
- votes are removed from back of queue if `queue.len() > 32`
- the earliest and latest height that has been removed from the back of the
  queue should be stored

It is likely that a reward will be offered as a % of the slashed amount to any
node that submits proof of this slashing condition being violated to the PoH.

### Partial Slashing

In the schema described so far, when a validator votes on a given PoH stream,
they are committing themselves to that fork for a time determined by the vote
lockout. An open question is whether validators will be hesitant to begin
voting on an available fork if the penalties are perceived too harsh for an
honest mistake or flipped bit.

One way to address this concern would be a partial slashing design that results
in a slashable amount as a function of either:

1. the fraction of validators, out of the total validator pool, that were also
   slashed during the same time period \(ala Casper\)
2. the amount of time since the vote was cast \(e.g. a linearly increasing % of
   total deposited as slashable amount over time\), or both.

This is an area currently under exploration.
