---
title: Epoch Reward Calculation
---

## Problem

Calculating and distributing rewards at one slot on the epoch boundary is not
scalable. With increasing number of stake accounts and validator nodes, the
block time at epoch boundary can last as long as 20 seconds. This degrades the
network performance and becomes a potential vulnerability of the network.

To solve this problem and speed up the reward at epoch boundary, an earlier
approach [Partitioned Inflationary Rewards
Distribution](https://github.com/solana-labs/solana/blob/master/docs/src/proposals/partitioned-inflationary-rewards-distribution.md)
is proposed. A prototype of this proposal is implemented. While this work solves
the scalability of epoch rewards, it, however, has two main problems:

1. Restrict updates to all stake accounts and vote account during reward
interval. In the earlier approach, the stake accounts and vote accounts are locked
during reward interval. All stake account manipulation such as delegate,
redelegate are not allowed.

1. No on-chain proof for rewards distribution at the epoch. In the earlier
approach, the rewards distribution is happening inside bank runtime. There is no
on-chian records for all rewards. All the rewards are in a black-box.

Therefore, we propose a new way to tackle epoch reward. The new method will
completely decouple reward calculation and reward distribution. In this
proposal, we will discuss "reward calculation", which will solve the first
problem. A next proposal will focus on "reward distribution", which will solve
the second problem.

## Proposed Solutions

Instead of computing the rewards at the first slot of epoch boundary, the reward
computation is moved to a background service.

It is very similar to the earlier proposal, a separate service,
"EpochRewardCalculationService" will be created. The service will listen to a
channel for any incoming rewards calculation requests, and perform the
calculation for the rewards.

The main difference is that instead of sharing the `StakeCache` with the runtime
during reward computation. The `StakeCache` is clone at slot-height 0 in the new
epoch. In this way, we don't need to lock the update for stake and vote account
during the reward calculation.

And the reward are computed for the next `N` slot-hight. At the
end of `Nth` slot-hight, the reward computation result should be available (the
slot will wait till the computation is finished).

At slot-height `N`, the total rewards from the calculation result, and the root
hash from a merkle tree, which consists of all the rewards for the epoch are
pushed to the blockchain. To achieve this, a new system account,
"EpochRewardProof" will be added. This system account contain a vector of accounts
(epoch, (reward_amount, root_hash)).

"EpochRewardProof" will be used in later for epoch reward distribution. See "Epoch
Reward Distribution" for more details.

### Challenges and Discussions

1. How hard to clone the stake cache? And what's the performance overhead for this?

1. How to adapt the current RPC method for user to the the rewards? What kind of new RPC API to provide for the user?

1. How to handle validator restart during reward calculation? Saving and loading stake_cache in snapshot?
