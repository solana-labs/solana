---
title: "Transaction Journey"
description: "Explore how each transaction starts with the user, processed by validators, and is finally included in a block and propagated to the rest of the network."
---

In this article we’ll explore a Solana transaction's journey through the network. We’ll discuss how each transaction starts with the user, gets processed by the validators, and is finally included in a block and propagated to the rest of the network.

Broadly speaking, a Solana transaction takes two primary steps to move through the network:

1. the client signs and submits the transaction to the cluster, then
2. it gets process by the transaction processing unit (TPU), where it will receive some validation and eventually broadcasted to the rest of the network

## Step 1 - The Client Submits the Transaction

A transaction's journey starts when a user initializes the transaction by signing it with their [private key](./../../terminology.md#private-key) (via a wallet). Once the transaction is signed, the client application forwards it to an RPC node. The RPC node is responsible for routing the transaction to the [validator](./../../terminology.md#validator) nodes for processing.

![A Solana transaction traveling to a slot Leader](/img/submitting-a-transaction.png)

After an RPC node receives a transaction, it will convert the transaction to a [QUIC packet](./../../validator/quic.md) before forwarding it to the relevant leaders (according to the [leader schedule](./../../cluster/leader-rotation.md)). A [leader](./../../terminology.md#leader) is the validator node responsible for generating a block at a particular moment in time.

> NOTE: Previously, Solana transactions were routed as UDP packets. But since [v1.13.4](https://github.com/solana-labs/solana/releases/tag/v1.13.4), the network upgraded to the QUIC protocol to improve stability and security.

## Step 2 - The Transaction Processing Unit (TPU)

After the RPC node receives a transaction, it looks up the leader schedule and sends the transaction as a network packet to the current and next leader’s transaction processing unit (TPU).

![an overview of the stages withing the TPU](/img/overview-of-tpu.png)

The TPU is responsible for processing the transactions and does so in the following four distinct phases:

- [Fetch stage](./../../validator/tpu.md#fetch-stage): allocates packet memory and reads the packet data from
  the network socket and applies some coalescing of packets received at
  the same time.

- [SigVerify stage](./../../validator/tpu.md#signature-verification-stage): de-duplicates packets and applies some load-shedding
  to remove excessive packets before then filtering packets with invalid
  signatures by setting the packet's discard flag.

- [Banking stage](./../../validator/tpu.md#banking-stage): decides whether to forward, hold or process packets
  received. Once it detects the node is the block producer it processes
  held packets and newly received packets with a Bank at the tip slot.

- [Broadcast stage](./../../validator/tpu.md#broadcast-stage): receives the valid transactions formed into
  an **entry** from banking stage and packages them into **shreds** to send to
  network peers through the [turbine](./../../cluster/turbine-block-propagation.md) tree structure.
  In the end, the Broadcast stage serializes, signs, and generates erasure codes before sending the packets to the appropriate network peer.

### Speed of Propagation

Using Turbine, it takes around **200ms** for a shred to propagate from the leader to **40,000** nodes. This propagation method significantly increases the throughput for transactions on the Solana network.

## Conclusion

The validators now all have a copy of the processed transaction—and that's the end of the transaction journey in Solana!
