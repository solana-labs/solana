---
title: Validator Frequently Asked Questions
sidebar_label: Frequently Asked Questions
---

### What is a validator?

A validator is a computer that runs a software program to verify transactions that are added to the Solana blockchain.  A validator can be a voting validator or a non voting validator. To learn more, see [what is a validator](./overview/what-is-a-validator.md).

### What is an RPC node?

An RPC node is also a computer that runs the validator software.  Typically, an RPC node does not vote on the network.  Instead the RPC node's job is to respond to API requests.  See [what is an rpc node](./overview/what-is-an-rpc-node.md) for more information.

### What is a cluster?

For a definition and an overview of the topic, see [what is a cluster?](../cluster/overview.md). Solana maintains several clusters. For details on each, see [Solana clusters](../clusters.md).

### What is Proof of Stake?

Proof of Stake (PoS) is a blockchain architecture. Solana is a Proof of Stake blockchain. To read more, see [Proof of Stake](./overview/what-is-a-validator.md#proof-of-stake).

### What is Proof of Work? Is running a Solana validator the same as mining?

No, a Solana validator uses Proof of Stake. It does not use Proof of Work (often called mining). See [Proof of Work: For Contrast](./overview/what-is-a-validator.md#proof-of-stake).

### Who can operate a validator?

Anyone can operate a validator.  All Solana clusters are permissionless. A new operator can choose to join at any time.

### Is there a validator set or limited number of validators that can operate?

No, all Solana clusters are permissionless.  There is no limit to the number of active validators that can participate in consensus.  Validators participating in consensus (voting validators) incur transaction fees for each vote.  A voting validator can expect to incur up to 1.1 SOL per day in vote transaction fees.

### What are the hardware requirements for running a validator?

See [validator requirements](../running-validator/validator-reqs.md).

### Can I run my validator at home?

Anyone can join the cluster including home users. You must make sure that your system can perform well and keep up with the cluster. Many home internet connections are not suitable to run a Solana validator.  Most operators choose to operate their validator in a data center either by using a server provider or by supplying your own hardware at a colocation data center.

See the [validator requirements](../running-validator/validator-reqs.md) for more information.

### What skills does a Solana validator operator need?

See [Solana validator prerequisites](./overview/validator-prerequisites.md).

### What are the economics of running a validator?

See [economics of running a validator](./overview/running-validator-or-rpc-node.md#economics-of-running-a-consensus-validator).