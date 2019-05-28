# Stake Delegation and Reward

This design proposal focuses on the software architecture for the on-chain
voting and staking programs.  Incentives for staking is covered in [staking
rewards](staking-rewards.md).

The current architecture requires a vote for each delegated stake from the
validator, and therefore does not scale to allow replicator clients to
automatically delegate their rewards.

The design proposes a new set of programs for voting and stake delegation, The
proposed programs allow many stake accounts to passively earn rewards with a
single validator vote without permission or active involvement from the
validator.

## Current Design Problems

In the current design each staker creates their own VoteState, and assigns a
**delegate** in the VoteState that can submit votes.  Since the validator has to
actively vote for each stake delegated to it, validators can censor stakes by
not voting for them.

The number of votes is equal to the number of stakers, and not the number of
validators.  Replicator clients are expected to delegate their replication
rewards as they are earned, and therefore the number of stakes is expected to be
large compared to the number of validators in a long running cluster.

## Proposed changes to the current design.

The general idea is that instead of the staker, the validator will own the
VoteState program. In this proposal the VoteState program is there to track
validator votes, count validator generated credits and to provide any
additional validator specific state.  The VoteState program is not aware of any
stakes delegated to it, and has no staking weight.

The rewards generated are proportional to the amount of lamports staked.  In
this proposal stake state is stored as part of the StakeState program. This
program is owned by the staker only.  Lamports stored in this program are the
stake.  Unlike the current design, this program contains a new field to indicate
which VoteState program the stake is delegated to.


### Benefits

* Single vote for all the stakers.

* Clearing of the credit variable is not necessary for claiming rewards.

* Each delegated stake can claim its rewards independently.

* Commission for the work is deposited when a reward is claimed by the delegated
stake.

This proposal would benefit from the `read-only` accounts proposal to allow for
many rewards to be claimed concurrently.

## Passive Delegation

Any number of instances of StakeState::Delegate programs can delegate to a single
VoteState program without an interactive action from the identity controlling
the VoteState program or submitting votes to the program.

The total stake allocated to a VoteState program can be calculated by the sum of
all the StakeState programs that have the VoteState pubkey as the
`StakeState::Delegate::voter_pubkey`.

## Example Callflow

<img alt="Passive Staking Callflow" src="img/passive-staking-callflow.svg" class="center"/>

## Future work

Validators may want to split the stake delegated to them amongst many validator
nodes since stake is used as weight in the network control and data planes.  One
way to implement this would be for the StakeState to delegate to a pool of
validators instead of a single one.

Instead of a single `vote_pubkey` and `credits_observed` entry in the StakeState
program, the program can be initialized with a vector of tuples.

```rust,ignore
Voter {
    voter_pubkey: Pubkey,
    credits_observed: u64,
    weight: u8,
}
```

* voters: Vec<Voter> - Array of VoteState accounts that are voting rewards with
this stake.

A StakeState program would claim a fraction of the reward from each voter in
the `voters` array, and each voter would be delegated a fraction of the stake.
