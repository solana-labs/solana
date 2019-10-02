# Slashing

This design describes how slashing is implemented in the solana
cluster.  Slashing is designed as a punishment for participants in
the cluster that violate the rules of the protocol.  A proof of the
violation is submitted to the cluster and if the proof is accepted,
the stake that is delegated to the node that violated a rule is
slashed by some percentage.

## Stake Slashing Percentage

Each stake will specify what percentage of the stake is slashed.
This percentage determines the amount of rewards the stake receives.

## Slashing Validator Lockouts

Validators submit votes which contain a list of slots.

* vote 0: 0
* vote 1: 0, 1
* vote 2: 0, 1, 2

The slots must match Tower consensus exactly.

* vote 3: 0, 1, 5

To submit a proof that the validator violated a consensus rule, the prover needs to submit two votes.

* vote 3: 0, 1, 5
* vote 4: 0, 1, 2, 3

Tower "0,1,5", "0,1,2,3" violate lockout rules and can be slashed.

