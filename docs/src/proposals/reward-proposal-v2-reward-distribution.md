---
title: Epoch Reward Distribution
---

## Problem

In `Epoch Reward Calculation`, we proposed a new method to handle epoch rewards.
This is the second part of that proposal. The main problem to address in this
proposal is the lack of on-chain proof for reward distribution.

## Proposed Solutions

Similar to [Partitioned Inflationary Rewards
Distribution](https://github.com/solana-labs/solana/blob/master/docs/src/proposals/partitioned-inflationary-rewards-distribution.md),
the rewards is distributed over `M` slots.

A special system instruction, `DistributeReward`, will be provided to distribute
the rewards from the `EpochRewardProof` system account. The instruction takes
the target account, the amount to distribute and the merkle proof for the
distribution. When executing this instruction, it first verify the supplied
merkle proof against the root hash before distributing the rewards.

For each slot, a fixed number (`K`) of stake/vote accounts's rewards are
distributed. Leader, within `M` slot-height, will be distributing the rewards by
injecting `K` `DistributeReward` transactions into the block. And bank runtime
will execute those transactions and distribute the rewards.

In this way, all reward distribution will be recoded in the block transactions.

### Challenges and Discussions

1. What are the incentives for the leader to pack reward redeem transactions?
   What kind of block fees will the leader receive for such transactions? What
   prevent a malicious leader not including those reward distribution
   transactions?

1. What's the performance overhead for packing transactions, executing reward
   distribute system instructions, and verifying merkle proof for rewards?

1. What's the performance impact for other transactions in the same block during
   reward distribution?

1. What's the performance impact for large number of reward transaction hitting
   the same account, i.e. "EpochRewardProof" account?

1.  How to handle validator restart during reward distribution? Saving reward
    calculation result and the entire merkle tree in snapshot?
