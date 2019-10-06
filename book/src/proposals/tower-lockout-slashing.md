# Tower Lockout Slashing

This design describes how slashing is implemented in the solana
cluster.  Slashing is designed as a punishment for participants in
the cluster that violate the rules of the protocol.  A proof of the
violation is submitted to the cluster and if the proof is accepted,
the stake that is delegated to the node that violated a rule is
slashed by some percentage.

## Votes

Validators submit votes which contain a list of slots.  To be
accepted, the vote must contain the list of slots currently in the
VoteState program, along with new votes.  The slots must match the
VoteState exactly.

* vote 0: 0
* vote 1: 0, 1
* vote 2: 0, 1, 2

## Proof that Validator violated a Lockout

To submit a proof that the validator violated a consensus rule, the
prover needs to submit two votes.

* vote 3: 0, 1, 2, 3
* vote 4: 0, 1, 2, 5

##  Accidental Slashing

A validator could submit the following vote:

* vote 3: 0, 1, 2, 3

Then crash, lose data, restart, and submit vote

* vote 5: 0, 1, 2, 5

Which would generate two votes that can be slashed, even though
they may be for non-conflicting forks.

###  Guarding Against Accidental Slashing

A Validator should stop voting but continue to retry old votes if
it does not observe the cluster accepting N votes.

If a validator is restarted and cannot recover the previous votes
from persistent storage, the validator should wait for 2^N slots
before resuming voting.  This would allow any pending votes to
expire before new votes are generated.

Before transmitting the vote to the network, validators should store
the pending votes in persistent storage.

## Variable Slashing Percentage

* vote 6: 0, 2, 4, 6
* vote 7: 1, 3, 5, 7

A validator continuously voting on two conflicting forks is doing
more damage to consensus then a validator that only violated the
lockout rules for a single block.

* Single block lockout should result in loss of rewards for the
epoch.

* Multiple block lockout results in slashing the validator as well
as loss of rewards for the epoch.

