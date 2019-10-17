# Tower Lockout Slashing

This design describes how slashing is implemented in the solana
cluster.  Slashing is designed as a punishment for participants in
the cluster that violate the rules of the protocol.  A proof of the
violation is submitted to the cluster and if the proof is accepted,
the stake that is delegated to the node that violated a rule is
slashed by some percentage.

## Votes

Validators submit votes which contain a list of slots.  To be
accepted, the vote must contain the list of N slots currently in
the VoteState program, along with new votes.

The first `N - new votes`, must match the VoteState after running
the tower consensus algorithm with the new votes.


N should be large enough to enforce a lockout that spans a full
epoch.  2^22 * 400ms is roughly 20 days, so 22 is a good choice for
N.

* vote 0: 0
* vote 1: 0, 1
* vote 2: 0, 1, 2


## Proof that Validator violated a Lockout

To submit a proof that the validator violated a consensus rule, the
prover needs to submit two votes.

* vote A: 0, 1, 2, 3
* vote B: 0, 1, 5

The second vote skips 3, and because 2's lockout is only 2, contains
the result of applying 5.  The same validator couldn't sign vote A
and B if they followed Tower consensus correctly.  To prove that a
lockout has been violated, the newest votes are applied to the
oldest vote.

* vote: 0, 1, 2, 3, 5

5 from vote B is applied to vote A.  If the result doesn't match
vote B, then a lockout has been violated.

##  Accidental Slashing

A validator could submit the following vote:

* vote 3: 0, 1, 2, 3

Then crash, lose data for vote 3, restart, and submit vote

* vote 5: 0, 1, 5

Which would generate two votes that can be slashed, even though
they may be for non-conflicting forks.

###  Guarding Against Accidental Slashing

A Validator should stop voting but continue to retry old votes if
it does not observe the cluster accepting N votes.  If the cluster
is not accepting old votes because the blockhash has expired, it
is safe for the validator to retry with a new blockhash, or wait
for 2^N slots until all the pending votes have expired.

If a validator is restarted and cannot recover the previous votes
from persistent storage, the validator should wait for 2^N slots
before resuming voting.  This would allow any pending votes to
expire before new votes are generated.

Before transmitting the vote to the network, validators should store
the pending votes in persistent storage.

* For this parameter, N should be on the order of the threshold
that validators use to observe a supermajority.

## Variable Slashing Percentage

* vote 6: 0, 2, 4, 6
* vote 7: 1, 3, 5, 7

A validator continuously voting on two conflicting forks is doing
more damage to consensus then a validator that only violated the
lockout rules for a single block.

* Single block lockout should result in loss of rewards for the
epoch and a minor slashing percentage.  This is a great candidate
for quadratic slashing, where valuators that vote on the same
single block have their slashing rate double.

* Multiple block lockout results in slashing the validator as well
as loss of rewards for the epoch.
