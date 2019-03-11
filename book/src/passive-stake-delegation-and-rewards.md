# Stake Delegation and Reward

This design proposal focuses on the software architecture for the on-chain
voting and staking programs.  Incentives for staking is covered in [staking
rewardsd](staking-rewards.md).

The current architecture requires an active vote for each delegated stake from
the validator, and therefore does not scale to allow for replicator clients
automatically delegate their rewards.
 
The design proposes a new set of programs for voting and stake delegation, The
proposed programs allow many stakes to passively earn rewards with a single
validator vote without permission from the validator.

## Current Design Problems

In the current design each staker creates their own VoteState, and assigns a
**delegate** in the VoteState that can submit votes.  Since the validator has to
actively vote for each stake delegated to it, it can censor stakes by not voting
for them.

The number of votes is equal to the number of stakers, and not the number of
validators.  Replicator clients are expected to automatically delegate their
rewards, and therefore the number of stakes is expected to be large compared to
the number of validators in a long running cluster.

## Proposed changes to the current design.

The general idea is that instead of the staker, the validator will own the
VoteState program. In this proposal the VoteState program is there to track
validator votes, count validator generated credits and to provide any
additional validator specific state.  The VoteState program is not aware of any
stakes delegated to it, and has no staking weight.

The rewards generated are proportional to the amount of lamports staked.  In
this proposal stake state is stored as part of the RewardsState program. This
program is owned by the staker only.  Lamports stored in this program are the
stake.  Unlike the current design, this program contains a new field to indicate
which VoteState program the stake is delegated to.

### New VoteState

VoteState is the current state of all the votes the **delegate** has submitted
to the bank.  VoteState contains the following state information:

* votes - The submitted votes data structure.

* credits - The total number of rewards this vote program generated over its
lifetime.

* root\_slot - The last slot to reach the full lockout commitment necessary for
rewards.

* commission - The commission taken by this VoteState for any rewards claimed by
staker's RewardsState accounts.  This is the percentage ceiling of the reward.

* Account::lamports - The accumulated lamports from the commission.  These do not
count as stakes.

* `authorized_voter_id` - Only this identity is authorized to submit votes.

### New RewardsState

RewardsState is the current delegation preference of the **staker**. RewardsState
contains the following state information:

* Account::lamports - The staked lamports.

* `voter_id` - The pubkey of the VoteState instance the lamports are
delegated to.

* `claimed_credits` - The total credits claimed over the lifetime of the
program.


### Claiming Rewards

The VoteState program and the RewardsState programs maintain a lifetime counter
of total rewards generated and claimed.  Therefore an explicit `Clear`
instruction is not necessary.  When claiming rewards, the total lamports
deposited into the RewardsState and as validator commission is proportional to
`VoteState::credits - RewardsState::claimed_credits`.

### RewardsInstruction::Initialize

* `account[0]` - Out Param - The RewardsState instance.  
  `RewardsState::claimed_credits` is initialized to `VoteState::credits`.  
  `RewardsState::voter_id` is initialized to `account[1]`

* `account[1]` - In Param - The VoteState instance.

### RewardsInstruction::RedeemVoteCredits


* `account[0]` - Out Param - The RewardsState instance.  

* `account[1]` - In Param - The VoteState instance, must be the same as
`RewardsState::voter_id`


Reward is payed out for the difference between `VoteState::credits` to
`RewardsState::claimed_credits`, and `claimed_credits` is updated to
`VoteState::credits`.  The commission is deposited into the `VoteState` token
balance, and the reward is deposited to the `RewardsState` token balance.  The
reward and the commission is weighted by the `RewardsState::lamports`.

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

## Passive Delegation

Any number of instances of RewardsState programs can delegate to a single
VoteState program without an interactive action from the identity controlling
the VoteState program or submitting votes to the program.

The total stake allocated to a VoteState program can be calculated by the sum of
all the Rewar:sState programs that have the VoteState pubkey as the
`RewardsState::voter_id`.
 
## Future work

Validators may want to split the stake delegated to them amongst many validator
nodes since stake is used as weight in the network control and data planes.  One
way to implement this would be for the RewardsState to delegate to a pool of
validators instead of a single one.

Instead of a single `vote_id` and `claimed_credits` entry in the RewardsState
program, the program can be initialized with a vector of tuples.

```
Voter {
    voter_id: Pubkey,
    claimed_credits: u64,
    weight: u8,
}
```

* voters: Vec<Voter> - Array of VoteState accounts that are voting rewards with
this stake.

A RewardsState program would claim a fraction of the reward from each voter in
the `voters` array, and each voter would be delegated a fraction of the stake.
