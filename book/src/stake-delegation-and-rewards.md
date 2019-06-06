# Stake Delegation and Rewards

Stakers are rewarded for helping to validate the ledger. They do this by
delegating their stake to validator nodes. Those validators do the legwork of
replaying the ledger and send votes to a per-node vote account to which stakers
can delegate their stakes.  The rest of the cluster uses those stake-weighted
votes to select a block when forks arise. Both the validator and staker need
some economic incentive to play their part. The validator needs to be
compensated for its hardware and the staker needs to be compensated for the risk
of getting its stake slashed.  The economics are covered in [staking
rewards](staking-rewards.md).  This chapter, on the other hand, describes the
underlying mechanics of its implementation.

## Basic Besign

The general idea is that the validator owns a Vote account. The Vote account
tracks validator votes, counts validator generated credits, and provides any
additional validator specific state.  The Vote account is not aware of any
stakes delegated to it and has no staking weight.

A separate Stake account (created by a staker) names a Vote account to which the
stake is delegated.  Rewards generated are proportional to the amount of
lamports staked.  The Stake account is owned by the staker only.  Lamports
stored in this account are the stake.

## Passive Delegation

Any number of Stake accounts can delegate to a single
Vote account without an interactive action from the identity controlling
the Vote account or submitting votes to the account.

The total stake allocated to a Vote account can be calculated by the sum of
all the Stake accounts that have the Vote account pubkey as the
`StakeState::Delegate::voter_pubkey`.

## Vote and Stake accounts

The rewards process is split into two on-chain programs. The Vote program solves
the problem of making stakes slashable. The Stake account acts as custodian of
the rewards pool, and provides passive delegation. The Stake program is
responsible for paying out each staker once the staker proves to the Stake
program that its delegate has participated in validating the ledger.

### VoteState

VoteState is the current state of all the votes the validator has submitted to
the network. VoteState contains the following state information:

* votes - The submitted votes data structure.

* credits - The total number of rewards this vote program has generated over its
lifetime.

* root\_slot - The last slot to reach the full lockout commitment necessary for
rewards.

* commission - The commission taken by this VoteState for any rewards claimed by
staker's Stake accounts.  This is the percentage ceiling of the reward.

* Account::lamports - The accumulated lamports from the commission.  These do not
count as stakes.

* `authorized_vote_signer` - Only this identity is authorized to submit votes. This field can only modified by this identity.

### VoteInstruction::Initialize

* `account[0]` - RW - The VoteState
  `VoteState::authorized_vote_signer` is initialized to `account[0]`
   other VoteState members defaulted

### VoteInstruction::AuthorizeVoteSigner(Pubkey)

* `account[0]` - RW - The VoteState
  `VoteState::authorized_vote_signer` is set to to `Pubkey`, instruction must by
   signed by Pubkey

### VoteInstruction::Vote(Vec<Vote>)

* `account[0]` - RW - The VoteState
  `VoteState::lockouts` and `VoteState::credits` are updated according to voting lockout rules see [Tower BFT](tower-bft.md)


* `account[1]` - RO - A list of some N most recent slots and their hashes for the vote to be verified against.


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
the pool can be split into several mining pools.  This design focuses on using
StakeState::MiningPool instances as the cluster wide mining pools.

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

The Staker or the owner of the Stake account sends a transaction with this
instruction to claim rewards.

The Vote account and the Stake account pair maintain a lifetime counter
of total rewards generated and claimed.  When claiming rewards, the total lamports
deposited into the Stake account and as validator commission is proportional to
`VoteState::credits - StakeState::credits_observed`.


* `account[0]` - RW - The StakeState::MiningPool instance that will fulfill the
reward.
* `account[1]` - RW - The StakeState::Delegate instance that is redeeming votes
credits.
* `account[2]` - R - The VoteState instance, must be the same as
`StakeState::voter_pubkey`

Reward is paid out for the difference between `VoteState::credits` to
`StakeState::Delgate.credits_observed`, and `credits_observed` is updated to
`VoteState::credits`.  The commission is deposited into the Vote account token
balance, and the reward is deposited to the Stake account token balance.

The total lamports paid is a percentage-rate of the lamports staked muiltplied by
the ratio of rewards being redeemed to rewards that could have been generated
during the rate period.

Any random MiningPool can be used to redeem the credits.

```rust,ignore
let credits_to_claim = vote_state.credits - stake_state.credits_observed;
stake_state.credits_observed = vote_state.credits;
```

`credits_to_claim` is used to compute the reward and commission, and
`StakeState::Delegate::credits_observed` is updated to the latest
`VoteState::credits` value.

## Collecting network fees into the MiningPool

At the end of the block, before the bank is frozen, but after it processed all
the transactions for the block, a virtual instruction is executed to collect
the transaction fees.

* A portion of the fees are deposited into the leader's account.
* A portion of the fees are deposited into the smallest StakeState::MiningPool
account.

## Authorizing a Vote Signer

`VoteInstruction::AuthorizeVoter` allows a staker to choose a signing service
for its votes. That service is responsible for ensuring the vote won't cause
the staker to be slashed.

## Benefits of the design

* Single vote for all the stakers.

* Clearing of the credit variable is not necessary for claiming rewards.

* Each delegated stake can claim its rewards independently.

* Commission for the work is deposited when a reward is claimed by the delegated
stake.

This proposal would benefit from the `read-only` accounts proposal to allow for
many rewards to be claimed concurrently.

## Example Callflow

<img alt="Passive Staking Callflow" src="img/passive-staking-callflow.svg" class="center"/>
