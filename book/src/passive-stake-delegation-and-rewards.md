# Stake Delegation and Reward

This design proposal focuses on the software architecture for the actual
on-chain programs.  Incentives for staking is covered in [staking
rewardsd](staking-rewards.md).


This proposal is solving is to allow many delegated stakes to passively earn
rewards with a single validator vote without permission from the validator.

The current architecture requires an active vote for each delegated stake from
the validator, and therefore does not scale to allow for replicator clients
automatically delegate their rewards.

## Current Problems

The current design requires a validator to submit a different vote for each
stake. Therefore the validator can censor stakes delegated to it, and the number
of votes is equal to the number of stakers, and not the number of validators.
Replicator clients are expected to automatically delegate their rewards, and
therefore the number of stakes is expected to be large compared to the number of
validators in a long running cluster.

## Terminology

* VoteState - Instance of the vote program.  This program keeps track of
validator votes.

* RewardState - Instance of the reward state program.  This program pays out
rewards for votes to the staker.

* Staker - The lamport owner that is risking lamports with consensus votes in
exchange for cluster rewards.  This is the owner of the RewardState program.

* Delegate - The validator that is submitting votes on behave of the staker.
This is the owner of the VoteState program.

## Proposal

The general idea is to store the delegation state in the reward program.  So
many RewardState instances can independently assign a single VoteState.  Each
RewardState can claim its reward independently with the VoteState program as
well as pay the VoteState program a commission.

VoteState instance is initialized by the validator.  RewardState is initialized
by the staker, and passively delegates the tokens stored in the RewardState to
an instance of the VoteState program.

### VoteState

VoteState is the current state of all the votes the **delegate** has submitted
to the bank.  VoteState contains the following state information:

* votes - The submitted votes.

* credits - The total number of rewards this vote program generated over its
lifetime.

* root\_slot - The last slot to reach the full lockout commitment necessary for
rewards.

* commission - The commission taken by this VoteState for any rewards claimed by
staker's RewardState accounts.

* lamports - The accumulated lamports from the commission.  These do not count as
stakes.

* `authorized_voter_id` - Only this identity is authorized to submit votes.

### RewardState

RewardState is the current delegation preference of the **staker**. RewardState
contains the following state information:

* lamports - The staked lamports.

* `vote_state_id` - The pubkey of the VoteState instance the lamports are
delegated to.

* `claimed_credits` - The total credits claimed over the lifetime of the
program.

## Passive Delegation

Any number of instances of RewardState programs can delegate to a single
VoteState program without an interactive action from the identity controlling
the VoteState program or submitting votes to the program.

The total stake allocated to a VoteState program can be calculated by the sum of
all the RewardState programs that have the VoteState pubkey as the
`RewardState;:vote_state_id`.
 
### RewardsInstruction::Initialize

* `account[0]` - Out Param - The RewardState instance.  
  `RewardState::claimed_credits` is initialized to `VoteState::credits`.  
  `RewardState::vote_state_id` is initialized to `account[1]`

* `account[1]` - In Param - The VoteState instance.

### RewardsInstruction::RedeemVoteCredits


* `account[0]` - Out Param - The RewardState instance.  

* `account[1]` - In Param - The VoteState instance, must be the same as
`RewardState::vote_state_id`


Reward is payed out for the difference between `VoteState::credits` to
`RewardState::claimed_credits`, and `claimed_credits` is updated to
`VoteState::credits`.  The commission is deposited into the `VoteState` token
balance, and the reward is deposited to the `RewardState` token balance.  The
reward and the commission is weighted by the `RewardState::lamports`.

The Staker, or the owner of the Reward program sends a transaction with this
instruction to claim the reward.

### Benefits

* Single vote for all the stakers.

* Clearing of the credit variable is not necessary for claiming rewards.

* Each delegated stake can claim its rewards independently.

* Commission for the work is deposited when a reward is claimed by the delegated
stake.

This proposal would benefit from the `read-only` accounts proposal to allow for
many rewards to be claimed concurrently.
