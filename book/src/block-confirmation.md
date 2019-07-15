# Block Confirmation

A validator votes on a PoH hash for two purposes. First, the vote indicates it
believes the ledger is valid up until that point in time. Second, since many
valid forks may exist at a given height, the vote also indicates exclusive
support for the fork. This document describes only the former. The latter is
described in [Tower BFT](tower-bft.md).

## Current Design

To start voting, a validator first registers an account to which it will send
its votes. It then sends votes to that account. The vote contains the tick
height of the block it is voting on. The account stores the 32 highest heights.

### Problems

* Only the validator knows how to find its own votes directly.

  Other components, such as the one that calculates confirmation time, needs to
  be baked into the fullnode code. The fullnode code queries the bank for all
  accounts owned by the vote program.

* Voting ballots do not contain a PoH hash. The validator is only voting that
  it has observed an arbitrary block at some height.

* Voting ballots do not contain a hash of the bank state. Without that hash,
  there is no evidence that the validator executed the transactions and
  verified there were no double spends.

## Proposed Design

### No Cross-block State Initially

At the moment a block is produced, the leader shall add a NewBlock transaction
to the ledger with a number of tokens that represents the validation reward.
It is effectively an incremental multisig transaction that sends tokens from
the mining pool to the validators. The account should allocate just enough
space to collect the votes required to achieve a supermajority. When a
validator observes the NewBlock transaction, it has the option to submit a vote
that includes a hash of its ledger state (the bank state). Once the account has
sufficient votes, the vote program should disperse the tokens to the
validators, which causes the account to be deleted.

#### Logging Confirmation Time

The bank will need to be aware of the vote program. After each transaction, it
should check if it is a vote transaction and if so, check the state of that
account. If the transaction caused the supermajority to be achieved, it should
log the time since the NewBlock transaction was submitted.

### Finality and Payouts

[Tower BFT](tower-bft.md) is the proposed fork selection algorithm.  It proposes
that payment to miners be postponed until the *stack* of validator votes reaches
a certain depth, at which point rollback is not economically feasible. The vote
program may therefore implement Tower BFT. Vote instructions would need to
reference a global Tower account so that it can track cross-block state.

## Challenges

### On-chain voting

Using programs and accounts to implement this is a bit tedious. The hardest
part is figuring out how much space to allocate in NewBlock. The two variables
are the *active set* and the stakes of those validators. If we calculate the
active set at the time NewBlock is submitted, the number of validators to
allocate space for is known upfront. If, however, we allow new validators to
vote on old blocks, then we'd need a way to allocate space dynamically.

Similar in spirit, if the leader caches stakes at the time of NewBlock, the
vote program doesn't need to interact with the bank when it processes votes. If
we don't, then we have the option to allow stakes to float until a vote is
submitted. A validator could conceivably reference its own staking account, but
that'd be the current account value instead of the account value of the most
recently finalized bank state. The bank currently doesn't offer a means to
reference accounts from particular points in time.

### Voting Implications on Previous Blocks

Does a vote on one height imply a vote on all blocks of lower heights of
that fork? If it does, we'll need a way to lookup the accounts of all
blocks that haven't yet reached supermajority. If not, the validator could
send votes to all blocks explicitly to get the block rewards.
