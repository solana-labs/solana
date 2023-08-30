---
title: Consensus Validator or RPC Node?
sidebar_label: Running a Validator or RPC Node?
---

Operators who run a [consensus validator](./what-is-a-validator.md) have much different incentives than operators who run an [RPC node](./what-is-an-rpc-node.md). You will have to decide which choice is best for you based on your interests, technical background, and goals.

## Consensus Validators

As a validator your primary focus is maintaining the network and making sure that your node is performing optimally so that you can fully participate in the cluster consensus. You will want to attract a delegation of SOL to your validator which will allow your validator the opportunity to produce more blocks and earn rewards.

Each staked validator earns inflation rewards from [vote credits](../../terminology.md#vote-credit). Vote credits are assigned to validators that vote on [blocks](../../terminology.md#block) produced by the [leader](../../terminology.md#leader). The vote credits are given to all validators that successfully vote on blocks that are added to the blockchain. Additionally, when the validator is the leader, it can earn transaction fees and storage [rent fees](../../developing/programming-model/accounts.md#rent) for each block that it produces that is added to the blockchain.

Since all votes in Solana happen on the blockchain, a validator incurs a transaction cost for each vote that it makes. These transaction fees amount to approximately 1.0 SOL per day.

> It is important to make sure your validator always has enough SOL in its identity account to pay for these transactions!

### Economics of running a consensus validator

As an operator, it is important to understand how a consensus validator spends and earns sol through the protocol.

All validators who vote (consensus validators) must pay vote transaction fees for blocks that they agree with.  The cost of voting can be up to 1.1 SOL per day.

A voting validator can earn SOL through 2 methods:

1. Inflationary rewards paid at the end of an epoch. See [staking rewards](./../../implemented-proposals/staking-rewards.md)
2. Earning 50% of transaction fees for the blocks produced by the validator.  See [transaction fee basic economic design](../../transaction_fees.md#basic-economic-design)

The following links are community provided resources that discuss the economics of running a validator:

- Michael Hubbard wrote an [article](https://laine-sa.medium.com/solana-staking-rewards-validator-economics-how-does-it-work-6718e4cccc4e) that explains the economics of Solana in more depth for stakers and for validators.
- Congent Crypto has written a [blog post](https://medium.com/@Cogent_Crypto/how-to-become-a-validator-on-solana-9dc4288107b7) that discusses economics and getting started.
- Cogent Crypto also provides a [validator profit calculator](https://cogentcrypto.io/ValidatorProfitCalculator)

## RPC Nodes

While RPC operators **do NOT** receive rewards (because the node is not participating in voting), there are different motivations for running an RPC node.

An RPC operator is providing a service to users who want to interact with the Solana blockchain. Because your primary user is often technical, you will have to be able to answer technical questions about performance of RPC calls. This option may require more understanding of the [core Solana architecture](../../cluster/overview.md).

If you are operating an RPC node as a business, your job will also involve scaling your system to meet the demands of the users. For example, some RPC providers create dedicated servers for projects that require a high volume of requests to the node. Someone with a background in development operations or software engineering will be a very important part of your team. You will need a strong understanding of the Solana architecture and the [RPC API](../../api/http.md).

Alternatively, you may be a development team that would like to run their own infrastructure. In this case, the RPC infrastructure could be a part of your production stack. A development team could use the [Geyser plugin](../../developing/plugins/geyser-plugins.md), for example, to get real time access to information about accounts or blocks in the cluster.