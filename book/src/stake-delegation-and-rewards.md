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

## Basic Design

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

The Stake account is moved from Uninitialized to StakeState::Stake form.  This is
how stakers choose their initial delegate validator node and activate their
stake account lamports.

* `account[0]` - RW - The StakeState::Stake instance. <br>
      `StakeState::Stake::credits_observed` is initialized to `VoteState::credits`,<br>
      `StakeState::Stake::voter_pubkey` is initialized to `account[1]`,<br>
      `StakeState::Stake::stake` is initialized to the u64 passed as an argument above,<br>
      `StakeState::Stake::activated` is initialized to current Bank epoch, and<br>
      `StakeState::Stake::deactivated` is initialized to std::u64::MAX

* `account[1]` - R - The VoteState instance.

* `account[2]` - R - sysvar::current account, carries information about current Bank epoch

* `account[3]` - R - stake_api::Config accoount, carries warmup, cooldown, and slashing configuration

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
* `account[3]` - R - sysvar::rewards account from the Bank that carries point value.
* `account[4]` - R - sysvar::stake_history account from the Bank that carries stake warmup/cooldown history

Reward is paid out for the difference between `VoteState::credits` to
`StakeState::Stake::credits_observed`, multiplied by `sysvar::rewards::Rewards::validator_point_value`.
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
* `account[1]` - R - The VoteState instance to which this stake is delegated, required in case of slashing
* `account[2]` - R - sysvar::current account from the Bank that carries current epoch

StakeState::Stake::deactivated is set to the current epoch + cool down.  The account's stake will ramp down to zero by
that epoch, and Account::lamports will be available for withdrawal.


### StakeInstruction::Withdraw(u64)
Lamports build up over time in a Stake account and any excess over activated stake can be withdrawn.

* `account[0]` - RW - The StakeState::Stake from which to withdraw, the transaction must be signed by this key.
* `account[1]` - RW - Account that should be credited with the withdrawn lamports.
* `account[2]` - R - sysvar::current account from the Bank that carries current epoch, to calculate stake.
* `account[3]` - R - sysvar::stake_history account from the Bank that carries stake warmup/cooldown history


## Benefits of the design

* Single vote for all the stakers.

* Clearing of the credit variable is not necessary for claiming rewards.

* Each delegated stake can claim its rewards independently.

* Commission for the work is deposited when a reward is claimed by the delegated
stake.

## Example Callflow

<img alt="Passive Staking Callflow" src="img/passive-staking-callflow.svg" class="center"/>

## Staking Rewards

The specific mechanics and rules of the validator rewards regime is outlined
here.  Rewards are earned by delegating stake to a validator that is voting
correctly.  Voting incorrectly exposes that validator's stakes to
[slashing](staking-and-rewards.md).

### Basics

The network pays rewards from a portion of network [inflation](inflation.md).
The number of lamports available to pay rewards for an epoch is fixed and
must be evenly divided among all staked nodes according to their relative stake
weight and participation.  The weighting unit is called a
[point](terminology.md#point).

Rewards for an epoch are not available until the end of that epoch.

At the end of each epoch, the total number of points earned during the epoch is
summed and used to divide the rewards portion of epoch inflation to arrive at a
point value.  This value is recorded in the bank in a
[sysvar](terminology.md#sysvar) that maps epochs to point values.

During redemption, the stake program counts the points earned by the stake for
each epoch, multiplies that by the epoch's point value, and transfers lamports in
that amount from a rewards account into the stake and vote accounts according to
the vote account's commission setting.

### Economics

Point value for an epoch depends on aggregate network participation.  If participation
in an epoch drops off, point values are higher for those that do participate.

### Earning credits

Validators earn one vote credit for every correct vote that exceeds maximum
lockout, i.e. every time the validator's vote account retires a slot from its
lockout list, making that vote a root for the node.

Stakers who have delegated to that validator earn points in proportion to their
stake.  Points earned is the product of vote credits and stake.

### Stake warmup, cooldown, withdrawal

Stakes, once delegated, do not become effective immediately.  They must first
pass through a warm up period.  During this period some portion of the stake is
considered "effective", the rest is considered "activating". Changes occur on
epoch boundaries.

The stake program limits the rate of change to total network stake, reflected
in the stake program's `config::warmup_rate` (typically 15% per epoch).

The amount of stake that can be warmed up each epoch is a function of the
previous epoch's total effective stake, total activating stake, and the stake
program's configured warmup rate.

Cooldown works the same way.  Once a stake is deactivated, some part of it
is considered "effective", and also "deactivating".  As the stake cools
down, it continues to earn rewards and be exposed to slashing, but it also
becomes available for withdrawal.

Bootstrap stakes are not subject to warmup.

Rewards are paid against the "effective" portion of the stake for that epoch.

#### Warmup example

Consider the situation of a single stake of 1,000 activated at epoch N, with
network warmup rate of 20%, and a quiescent total network stake at epoch N of 2,000.

At epoch N+1, the amount available to be activated for the network is 400 (20%
of 200), and at epoch N, this example stake is the only stake activating, and so
is entitled to all of the warmup room available.


|epoch | effective | activating | total effective | total activating|
|------|----------:|-----------:|----------------:|----------------:|
|N-1   |           |            |  2,000          |  0              |
|N     |  0        | 1,000      |  2,000          |  1,000          |
|N+1   |  400      | 600        |  2,400          |  600            |
|N+2   |  880      | 120        |  2,880          |  120            |
|N+3   |  1000     | 0          |  3,000          |  0              |


Were 2 stakes (X and Y) to activate at epoch N, they would be awarded a portion of the 20%
in proportion to their stakes.  At each epoch effective and activating for each stake is
a function of the previous epoch's state.

|epoch | X eff     | X act      | Y eff     | Y act      | total effective | total activating|
|------|----------:|-----------:|----------:|-----------:|----------------:|----------------:|
|N-1   |           |            |           |            |  2,000          |  0              |
|N     |  0        | 1,000      |  0        | 200        |  2,000          |  1,200          |
|N+1   |  320      | 680        |  80       | 120        |  2,400          |    800          |
|N+2   |  728      | 272        |  152      | 48         |  2,880          |    320          |
|N+3   |  1000     | 0          |  200      | 0          |  3,200          |      0          |


### Withdrawal

As rewards are earned lamports can be withdrawn from a stake account.  Only
lamports in excess of effective+activating stake may be withdrawn at any time.
This means that during warmup, effectively no stake can be withdrawn.  During
cooldown, any tokens in excess of effective stake may be withdrawn (activating == 0);

### Lock-up

Stake accounts support the notion of lock-up, wherein the stake account balance is 
unavailable for withdrawal until a specified time.  Lock-up is specified as a slot height,
i.e. the minimum slot height that must be reached by the network before the stake account balance 
is available for withdrawal.
