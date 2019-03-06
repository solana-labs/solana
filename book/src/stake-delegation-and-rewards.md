# Stake Delegation and Reward

This design covers the current architecture to how stakes are delegated to
validators.  This design focuses on the software architecture for the actual
on-chain programs.  Incentives for staking is covered in [staking
rewardsd](staking-rewards.md).

The main problem this proposal is solving is to allow many delegated stakes to
generate rewards with a single validator vote without permission from the
validator.

## Terminology

* VoteState - Instance of the vote program.  This program keeps track of
validator votes.

* RewardState - Instance of the reward state program.  This program pays out
rewards for votes to the staker.

* Staker - The lamport owner that is risking lamports with consensus votes in
exchange for network rewards.

* Delegate - The validator that is submitting votes on behave of the staker.

## Current Architecture

VoteState contains the following state information:

* votes - The submitted votes.

* `delegate_id` - The delegated stake identity.  This identity can submit votes
as well.

* `credits` - The amount of unclaimed rewards.

* `root_slot` - The last slot to reach the full lockout commitment necessary for
rewards.

Reward program is stateless, and pays out the reward when that reward is
claimed.  Claim the reward requires a transaction that includes the following
instructions:

1. RewardsInstruction::RedeemVoteCredits

2. VoteInstruction::ClearCredits

The payout of the rewards transfers lamports from the rewards program to the
VoteState address. It ensures that VoteState clears its credits in the next
instruction.

### VoteInstruction::DelegateStake(Pubkey)

This instruction assigns the delegate\_id to the supplied Pubkey.  The
delegation allows the delegate\_id to submit votes on behalf of the vote
program.

## Validator work flow.

A validator that has been delegated N stakes needs to submit N votes, and the
validator must submit each vote, for each of the delegated state to earn a
reward.  We expect each validator to vote once per block, and for there to be
much more delegated stakes in number then validators.  With the current design,
each delegated stake requires an independent vote.
