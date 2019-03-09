# Staking Accounts

The [Stake Delegation and Rewards] achitecture won't scale well when many
stakers delegate their stakes to the same fullnode. The fullnode must send a
separate vote to each staking account. If there are far more stakers than
fullnodes, that's a lot of redundant network traffic. The problem stems from
the assumption that slashing could only be implemented from the the account
used to collect votes and it therefore must hold the stake.  That assumption
doesn't hold water. Stake only needs to be tied to the account that implements
slashing conditions. A separate voting account can be submitted to the staking
account as evidence that the stake must be slashed. This proposal splits
today's `Vote` account into two accounts, such that a VoteSigner only needs to
submit votes to one account, an account which it created itself.

## The Stake program

Today's Vote program and its VoteState should be split into two. A new Stake
program will host the `delegate_id` and `authorized_voter_id` in a new struct
called StakeState, while all Locktower state will remain in VoteState. The
`authorized_voter_id` field should be renamed to `voter_id`, indicating no
authorization is required for the voter to submit votes.

In VoteState, `credits` should be replaced with `num_votes_popped`, a
monotonically increasing counter that is initialized to zero with the VoteState
is created. StakeState should also have a `num_votes_popped` that's updated to
`VoteState::num_votes_popped` when `StakeState::voter_id` is set and any time
the staker goes to the Rewards program to claim a reward. Both the staker and
voter should be rewarded for each vote popped during the time that the staker
backed the voter.

## Changes to the Rewards program

The Rewards program will need a few small changes. Currently, the owner of the
voting account signs a transaction with a
`RewardsInstruction::RedeemVoteCredits` and `VoteInstruction::ClearCredits`.
That should be replaced with a transaction signed by the staking account
including a `RewardsInstruction::ClaimVotingRewards` and a
`StakeInstruction::UpdateVotesPopped`. The Rewards program should add lamports
to both the staking and voting accounts. How much exactly is outside the scope
of this document.

