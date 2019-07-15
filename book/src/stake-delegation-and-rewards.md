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
lamports staked.  The Stake account is owned by the staker only.  Some portion of the lamports
stored in this account are the stake.

## Passive Delegation

Any number of Stake accounts can delegate to a single
Vote account without an interactive action from the identity controlling
the Vote account or submitting votes to the account.

The total stake allocated to a Vote account can be calculated by the sum of
all the Stake accounts that have the Vote account pubkey as the
`StakeState::Stake::voter_pubkey`.

## Vote and Stake accounts

The rewards process is split into two on-chain programs. The Vote program solves
the problem of making stakes slashable. The Stake account acts as custodian of
the rewards pool, and provides passive delegation. The Stake program is
responsible for paying out each staker once the staker proves to the Stake
program that its delegate has participated in validating the ledger.

### VoteState

VoteState is the current state of all the votes the validator has submitted to
the network. VoteState contains the following state information:

* `votes` - The submitted votes data structure.

* `credits` - The total number of rewards this vote program has generated over its
lifetime.

* `root_slot` - The last slot to reach the full lockout commitment necessary for
rewards.

* `commission` - The commission taken by this VoteState for any rewards claimed by
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
  `VoteState::authorized_vote_signer` is set to to `Pubkey`, the transaction must by
   signed by the Vote account's current `authorized_vote_signer`.  <br>
   `VoteInstruction::AuthorizeVoter` allows a staker to choose a signing service
for its votes. That service is responsible for ensuring the vote won't cause
the staker to be slashed.


### VoteInstruction::Vote(Vec<Vote>)

* `account[0]` - RW - The VoteState
  `VoteState::lockouts` and `VoteState::credits` are updated according to voting lockout rules see [Tower BFT](tower-bft.md)


* `account[1]` - RO - A list of some N most recent slots and their hashes for the vote to be verified against.


### StakeState

A StakeState takes one of three forms, StakeState::Uninitialized, StakeState::Stake and StakeState::RewardsPool.

### StakeState::Stake

StakeState::Stake is the current delegation preference of the **staker** and
contains the following state information:

* Account::lamports - The lamports available for staking.

* `stake` - the staked amount (subject to warm up and cool down) for generating rewards, always less than or equal to Account::lamports

* `voter_pubkey` - The pubkey of the VoteState instance the lamports are
delegated to.

* `credits_observed` - The total credits claimed over the lifetime of the
program.

* `activated` - the epoch at which this stake was activated/delegated. The full stake will be counted after warm up.

* `deactivated` - the epoch at which this stake will be completely de-activated, which is `cool down` epochs after StakeInstruction::Deactivate is issued.

### StakeState::RewardsPool

To avoid a single network wide lock or contention in redemption, 256 RewardsPools are part of genesis under pre-determined keys, each with std::u64::MAX credits to be able to satisfy redemptions according to point value.

The Stakes and the RewardsPool are accounts that are owned by the same `Stake` program.

### StakeInstruction::DelegateStake(u64)

The Stake account is moved from Unitialized to StakeState::Stake form.  This is
how stakers choose their initial delegate validator node and activate their
stake account lamports.

* `account[0]` - RW - The StakeState::Stake instance. <br>
      `StakeState::Stake::credits_observed` is initialized to `VoteState::credits`,<br>
      `StakeState::Stake::voter_pubkey` is initialized to `account[1]`,<br>
      `StakeState::Stake::stake` is initialized to the u64 passed as an argument above,<br>
      `StakeState::Stake::activated` is initialized to current Bank epoch, and<br>
      `StakeState::Stake::deactivated` is initialized to std::u64::MAX

* `account[1]` - R - The VoteState instance.

* `account[2]` - R - syscall::current account, carries information about current Bank epoch

### StakeInstruction::RedeemVoteCredits

The staker or the owner of the Stake account sends a transaction with this
instruction to claim rewards.

The Vote account and the Stake account pair maintain a lifetime counter of total
rewards generated and claimed.  Rewards are paid according to a point value
supplied by the Bank from inflation.  A `point` is one credit * one staked
lamport, rewards paid are proportional to the number of lamports staked.

* `account[0]` - RW - The StakeState::Stake instance that is redeeming rewards.
* `account[1]` - R - The VoteState instance, must be the same as `StakeState::voter_pubkey`
* `account[2]` - RW - The StakeState::RewardsPool instance that will fulfill the request (picked at random).
* `account[3]` - R - syscall::rewards account from the Bank that carries point value.

Reward is paid out for the difference between `VoteState::credits` to
`StakeState::Stake::credits_observed`, multiplied by `syscall::rewards::Rewards::validator_point_value`.
`StakeState::Stake::credits_observed` is updated to`VoteState::credits`.  The commission is deposited into the Vote account token
balance, and the reward is deposited to the Stake account token balance.


```rust,ignore
let credits_to_claim = vote_state.credits - stake_state.credits_observed;
stake_state.credits_observed = vote_state.credits;
```

`credits_to_claim` is used to compute the reward and commission, and
`StakeState::Stake::credits_observed` is updated to the latest
`VoteState::credits` value.

### StakeInstruction::Deactivate
A staker may wish to withdraw from the network.  To do so he must first deactivate his stake, and wait for cool down.

* `account[0]` - RW - The StakeState::Stake instance that is deactivating, the transaction must be signed by this key.
* `account[1]` - R - syscall::current account from the Bank that carries current epoch

StakeState::Stake::deactivated is set to the current epoch + cool down.  The account's stake will ramp down to zero by
that epoch, and Account::lamports will be available for withdrawal.


### StakeInstruction::Withdraw(u64)
Lamports build up over time in a Stake account and any excess over activated stake can be withdrawn.

* `account[0]` - RW - The StakeState::Stake from which to withdraw, the transaction must be signed by this key.
* `account[1]` - RW - Account that should be credited with the withdrawn lamports.
* `account[2]` - R - syscall::current account from the Bank that carries current epoch, to calculate stake.


## Benefits of the design

* Single vote for all the stakers.

* Clearing of the credit variable is not necessary for claiming rewards.

* Each delegated stake can claim its rewards independently.

* Commission for the work is deposited when a reward is claimed by the delegated
stake.

## Example Callflow

<img alt="Passive Staking Callflow" src="img/passive-staking-callflow.svg" class="center"/>
