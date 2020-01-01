# Leader Duplicate Block Slashing

This design describes how the cluster slashes leaders that produce
duplicate blocks.

Leaders that produce multiple blocks for the same slot increase the
number of potential forks that the cluster has to resolve.

## Overview

1. Slashing Condition: If you have voted on version `S1` of a slot, it is a slashing condition to vote on any descendant of another version `S2` of the slot, unless that descendant of version `S2` has seen supermajority (> 66%) of the stake voting on it (See section `Slashing Condition` below for how this proof is to be generated).
2. If you see two blockhashes for the same block, and you are less than 2^THRESHOLD lockout on that block, drop it. Flag the block as "duplicated".
3. Only repair a version of the "duplicated" block if you see a `Repair Proof` as outlined in the `Repair Proof` section below

## Primitives

For a bank `B`, let `A` be the latest ancestor of `B` that has gotten supermajority > 66% votes. 

1) Define the function `Confirmed` to be `Confirmed(B) = A`. 
2) Define `parent(B)` to be the parent bank of `B`.

The `bank_hash` of each bank `B` is now augmented to be: 

`hash(Confirmed(A).slot, state_hash(B), bank_hash(parent(B)), parent(B).slot)`.

## Repair Proof

Let `S'` be some version of slot `S` that a validator has already detected a
duplicate version of and dropped and marked as "duplicate"
(step 2 of the `Overview` section above).  A validator will only repair 
`S'` if it's provided a proof that `S'` has been confirmed 
(been voted on by greater than 66% of the stake). 

Call such a proof `RepairProof(S')`.

This proof consists of a supermajority of validator's votes, where each vote `V`:

* Contains a bank hash `H(B)` for some bank `B` such that `Confirmed(B).slot < S'.slot` (if a supermajority confirmed `S'`, then such a set must exist because there must have been some initial set of validators that voted on `S'` before it was confrmed). This can be confirmed for each vote if the prover provides `state_hash(B)`, `Confirmed(B).slot`, `bank_hash(parent(B))`, `parent(B).slot` such that `H(B) == hash(Confirmed(B).slot, state_hash(B), bank_hash(parent(B)), parent(B).slot)`.

* Show that `V.slot` is descended from `S'` by proving the bank hash `H(B)` can be derived from the bank hash and slot of `S'`. This is done by providing the trail of bank hashes and parent slots to 
recreate the hash.

## Slashing Condition

### Goals
The proof of slashing aims to show a validator signed two votes on two bank hashes `H1` and `H2` for two banks `B1` and `B2` where both:
* `Confirmed(B1) < S.slot` and `Confirmed(B2) < S.slot`.
* Chain from two different versions of some slot `S`

### Contents of the Proof
Let `S1` and `S2` be two versions of some slot `S`. A proof shows two signed votes for two 
banks `B1` and `B2` with bank hashes `H(B1)` and `H(B2)`. The proof shows:

* `Proof of Minority`: For each of these hashes `H` and each bank `B`, show that `Confirmed(B) < S` by providing `state_hash(B)`, `Confirmed(B).slot`, `bank_hash(parent(B))`, `parent(B).slot` such that:

`H(B) == hash(Confirmed(B).slot, state_hash(B), bank_hash(parent(B)), parent(B).slot)`.

* `Proof of Chaining`: Prove that both banks are descended from a different version of `S.slot`

From the protocol outlined in `Conditions for Repairing a Slot with Multiple Versions`, if a validator
is shown valid `Repair Proof(S1)` and `Repair Proof(S2)`, then that means at least 33%
of the validators must have votes for different versions of `S` in both repair proofs. We can then generate a slashing proof for these 33% by comparing the overlapping validator's votes that were
contained in both repair proofs (Both `Proof of Minority` and the conflicting `Proof of Chaining` will exist in the two `Repair Proof`'s).

### Guarantees:

1) If a correct validator in the cluster is locked out greater than 2^THRESHOLD on a version "A" of a block, then that version is the only version that correct validators will repair, unless at least 33% of validators equivocate and get slashed
2) If no correct valdiator is locked out greater than 2^THRESHOLD, then everybody must have dropped the block and picked another fork


