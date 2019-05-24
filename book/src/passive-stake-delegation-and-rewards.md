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

### VoteState

VoteState is the current state of all the votes the **delegate** has submitted
to the bank.  VoteState contains the following state information:

* votes - The submitted votes data structure.

* credits - The total number of rewards this vote program has generated over its
lifetime.

* root\_slot - The last slot to reach the full lockout commitment necessary for
rewards.

* commission - The commission taken by this VoteState for any rewards claimed by
staker's StakeState accounts.  This is the percentage ceiling of the reward.

* Account::lamports - The accumulated lamports from the commission.  These do not
count as stakes.

* `authorized_vote_signer` - Only this identity is authorized to submit votes, and
this field can only modified by this entity

### VoteInstruction::Initialize

* `account[0]` - RW - The VoteState
  `VoteState::authorized_vote_signer` is initialized to `account[0]`
   other VoteState members defaulted

### VoteInstruction::AuthorizeVoteSigner(Pubkey)

* `account[0]` - RW - The VoteState
  `VoteState::authorized_vote_signer` is set to to `Pubkey`, instruction must by
   signed by Pubkey


### StakeState

A StakeState takes one of two forms, StakeState::Delegate and StakeState::MiningPool.

### StakeState::Delegate

StakeState is the current delegation preference of the **staker**. StakeState
contains the following state information:

* Account::lamports - The staked lamports.

* `voter_pubkey` - The pubkey of the VoteState instance the lamports are
delegated to.

* `credits_observed` - The total credits claimed over the lifetime of the
program.

### StakeState::MiningPool

There are two approaches to the mining pool.  The bank could allow the
StakeState program to bypass the token balance check, or a program representing
the mining pool could run on the network.  To avoid a single network wide lock,
the pool can be split into several mining pools.  This design focuses on using a
StakeState::MiningPool as the cluster wide mining pools.

* 256 StakeState::MiningPool are initialized, each with 1/256 number of mining pool
tokens stored as `Account::lamports`.

The stakes and the MiningPool are accounts that are owned by the same `Stake`
program.

### StakeInstruction::Initialize

* `account[0]` - RW - The StakeState::Delegate instance.
  `StakeState::Delegate::credits_observed` is initialized to `VoteState::credits`.
  `StakeState::Delegate::voter_pubkey` is initialized to `account[1]`

* `account[1]` - R - The VoteState instance.

### StakeInstruction::RedeemVoteCredits

The VoteState program and the StakeState programs maintain a lifetime counter
of total rewards generated and claimed.  Therefore an explicit `Clear`
instruction is not necessary.  When claiming rewards, the total lamports
deposited into the StakeState and as validator commission is proportional to
`VoteState::credits - StakeState::credits_observed`.


* `account[0]` - RW - The StakeState::MiningPool instance that will fulfill the
reward.
* `account[1]` - RW - The StakeState::Delegate instance that is redeeming votes
credits.
* `account[2]` - R - The VoteState instance, must be the same as
`StakeState::voter_pubkey`

Reward is payed out for the difference between `VoteState::credits` to
`StakeState::Delgate.credits_observed`, and `credits_observed` is updated to
`VoteState::credits`.  The commission is deposited into the `VoteState` token
balance, and the reward is deposited to the `StakeState::Delegate` token balance.  The
reward and the commission is weighted by the `StakeState::lamports` divided by total lamports staked.

The Staker or the owner of the Stake program sends a transaction with this
instruction to claim the reward.

Any random MiningPool can be used to redeem the credits.

```rust,ignore
let credits_to_claim = vote_state.credits - stake_state.credits_observed;
stake_state.credits_observed = vote_state.credits;
```

`credits_to_claim` is used to compute the reward and commission, and
`StakeState::Delegate::credits_observed` is updated to the latest
`VoteState::credits` value.

### Collecting network fees into the MiningPool

At the end of the block, before the bank is frozen, but after it processed all
the transactions for the block, a virtual instruction is executed to collect
the transaction fees.

* A portion of the fees are deposited into the leader's account.
* A portion of the fees are deposited into the smallest StakeState::MiningPool
account.

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
