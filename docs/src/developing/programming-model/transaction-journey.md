---
title: "Transaction Journey"
description: ""
---

In this article we’ll explore a Solana transaction's journey through the network. We’ll discuss how each transaction starts with the user, gets processed by the validators, and is finally included in a block and propagated to the rest of the network.

Broadly speaking, a Solana transaction takes two primary steps to move through the network:

1. the client signs and submits the transaction to the cluster, then
2. it gets process by the transaction processing unit (TPU), where it will receive some validation and eventually broadcasted to the rest of the network

## Step 1 - The Client Submits the Transaction

A transaction's journey starts when a user initializes the transaction by signing it with their [private key](./../../terminology.md#private-key) (via a wallet). Once the transaction is signed, the client application forwards it to an RPC node. The RPC node is responsible for routing the transaction to the [validator](./../../terminology.md#validator) nodes for processing.

![A Solana transaction traveling to a slot Leader](/img/submitting-a-transaction.png)

After an RPC node receives a transaction, it will convert the transaction to a [QUIC packet](./../../validator/quic.md) before forwarding it to the relevant leaders (according to the [leader schedule](https://docs.solana.com/cluster/leader-rotation)). A [leader](./../../terminology.md#leader) is the validator node responsible for generating a block at a particular moment in time.

> NOTE: Previously, Solana transactions were routed as UDP packets. But since v1.13.4, the network upgraded to the QUIC protocol to improve stability and security.

## Step 2 - The Transaction Processing Unit (TPU)

After the RPC node receives a transaction, it looks up the leader schedule and sends the transaction as a network packet to the current and next leader’s transaction processing unit (TPU).

The TPU is responsible for processing the transactions and does so in the following four distinct phases:

![an overview of the stages withing the TPU](/img/overview-of-tpu.png)

### Fetch Stage

Within the [Fetch Stage](#fetch-stage), validators categorize incoming transactions into three network sockets, each with a dedicated socket for each packet type:

- The [`tpu`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/gossip/src/contact_info.rs#L29) socket handles normal transactions such as token transfers, NFT mints, and program instructions.
- The [`tpu_forwards`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/gossip/src/contact_info.rs#L31) socket is responsible for forwarding unprocessed packets to the next leader if the current leader is unable to process all transactions.
- The [`tpu_vote`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/gossip/src/contact_info.rs#L33) socket focuses exclusively on validator votes. The network also views validator consensus votes as transactions and are transmitted as network packets.

The packets are batched together, with each [batch containing 64 packets](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/perf/src/packet.rs#L18) and forwarded to the [Signature Verification Stage](#signature-verification-stage).

### Signature Verification Stage

As we’ve seen, users must first sign the transaction before a user sends a transaction for processing. The [`SigVerifyStage`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/core/src/sigverify_stage.rs#L53) is where the signatures are [verified](https://github.com/solana-labs/solana/blob/cd6f931223181d5a1d47cba64e857785a175a760/core/src/sigverify.rs#L44) before the packets are forwarded to the next stage for processing. The node receives a list of packets and outputs the same list, but flags it telling the next stage whether the signature in that packet is valid.

### Banking Stage

The [Banking Stage](#banking-stage) decides whether to forward, hold, or process packets received. In this stage, the TPU runs each packet through the following steps:

- Deserializes the packet into transactions.
- Runs the transactions through the QoS model we mentioned earlier. The QoS prioritizes which transactions to process depending on factors such as the [transaction cost](./../../transaction_fees.md) and the stake weight of the forwarding node.
- The pipeline batches the parallelizable transactions—those that don’t write to the same state simultaneously—and executes them in parallel.

At this point, the transaction’s processing is done and the leader includes the transaction on its block before forwarding the block to the next stage for broadcast to the rest of the network.

### Broadcast Stage

In the Broadcast Stage, the challenge is to get the block from the leader node communicated to the validator nodes, and in a way that doesn’t congest the network and bring throughput to a crawl. For this, Solana uses a block propagation strategy called [Turbine](./../../cluster/turbine-block-propagation.md). The block is divided into small units called shreds and each shred is broadcasted to the rest of the network via a unique path.

You can think of the network as a tree of nodes. The first node transmits the shred to a specified number of nodes in a layer below it. Each of these nodes retransmits the shred to two other nodes in the layer below them.

#### Speed of Propagation

Using Turbine, it takes around **200ms** for a shred to propagate from the leader to **40,000** nodes. This propagation method significantly increases the throughput for transactions on the Solana network.

## Conclusion

The validators now all have a copy of the processed transaction—and that's the end of the transaction journey in Solana!
