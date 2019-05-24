# Stake Delegation and Rewards

Stakers are rewarded for helping validate the ledger. They do it by delegating
their stake to fullnodes. Those fullnodes do the legwork and send votes to the
stakers' staking accounts. The rest of the cluster uses those stake-weighted
votes to select a block when forks arise. Both the fullnode and staker need
some economic incentive to play their part. The fullnode needs to be
compensated for its hardware and the staker needs to be compensated for risking
getting its stake slashed.  The economics are covered in [staking
rewards](staking-rewards.md).  This chapter, on the other hand, describes the
underlying mechanics of its implementation.

## Vote and Rewards accounts

The rewards process is split into two on-chain programs. The Vote program
solves the problem of making stakes slashable. The Rewards account acts as
custodian of the rewards pool. It is responsible for paying out each staker
once the staker proves to the Rewards program that it participated in
validating the ledger.

The Vote account contains the following state information:

* votes - The submitted votes.

* `delegate_pubkey` - An identity that may operate with the weight of this
  account's stake. It is typically the identity of a fullnode, but may be any
identity involved in stake-weighted computations.

* `authorized_voter_pubkey` - Only this identity is authorized to submit votes.

* `credits` - The amount of unclaimed rewards.

* `root_slot` - The last slot to reach the full lockout commitment necessary
  for rewards.

The Rewards program is stateless and pays out reward when a staker submits its
Vote account to the program. Claiming a reward requires a transaction that
includes the following instructions:

1. `RewardsInstruction::RedeemVoteCredits`
2. `VoteInstruction::ClearCredits`

The Rewards program transfers lamports from the Rewards account to the Vote
account's public key. The Rewards program also ensures that the `ClearCredits`
instruction follows the `RedeemVoteCredits` instruction, such that a staker may
not claim rewards for the same work more than once.


### Delegating Stake

`VoteInstruction::DelegateStake` allows the staker to choose a fullnode to
validate the ledger on its behalf. By being a delegate, the fullnode is
entitled to collect transaction fees when its is leader. The larger the stake,
the more often the fullnode will be able to collect those fees.

### Authorizing a Vote Signer

`VoteInstruction::AuthorizeVoter` allows a staker to choose a signing service
for its votes. That service is responsible for ensuring the vote won't cause
the staker to be slashed.

## Limitations

Many stakers may delegate their stakes to the same fullnode. The fullnode must
send a separate vote to each staking account. If there are far more stakers
than fullnodes, that's a lot of network traffic. An alternative design might
have fullnodes submit each vote to just one account and then have each staker
submit that account along with their own to collect its reward.
