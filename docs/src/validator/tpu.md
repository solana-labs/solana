---
title: Transaction Processing Unit (TPU)
sidebar_label: TPU
description: ""
---

Within the Solana validators, the [Transaction Processing Unit (TPU)](./tpu.md) is the logic that is responsible for receiving transactions from the client and eventually forming [blocks](./../terminology.md#block) that get broadcast to the network.

After an RPC node receives a transaction, it looks up the [leader schedule](./../cluster/leader-rotation.md) and sends the transaction as a [QUIC](./quic.md) network packet to the current and next leader’s TPU for processing.

![TPU Block Diagram](/img/tpu.svg)

## Stages of the TPU

The TPU processes transactions in the following four distinct phases:

- [Fetch stage](#fetch-stage): allocates packet memory and reads the packet data from
  the network socket and applies some coalescing of packets received at
  the same time.

- [SigVerify stage](#signature-verification-stage): de-duplicates packets and applies some load-shedding
  to remove excessive packets before then filtering packets with invalid
  signatures by setting the packet's discard flag.

- [Banking stage](#banking-stage): decides whether to forward, hold or process packets
  received. Once it detects the node is the block producer it processes
  held packets and newly received packets with a Bank at the tip slot.

- [Broadcast stage](#broadcast-stage): receives the valid transactions formed into [entries](./../terminology.md#entry)
  from banking stage and packages them into shreds to send to network peers through
  the turbine tree structure. In the end, this stage serializes, signs, and generates erasure codes
  before sending the packets to the appropriate network peer.

## Fetch Stage

Within the [Fetch Stage](#fetch-stage), validators categorize incoming transactions into three network sockets, each with a dedicated socket for each packet type:

- The [`tpu`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/gossip/src/contact_info.rs#L29) socket handles normal transactions such as token transfers, NFT mints, and program instructions.

- The [`tpu_forwards`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/gossip/src/contact_info.rs#L31) socket is responsible for forwarding unprocessed packets to the next leader if the current leader is unable to process all transactions.

- The [`tpu_vote`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/gossip/src/contact_info.rs#L33) socket focuses exclusively on validator votes. The network also views validator consensus votes as transactions and are transmitted as network packets.

The packets are batched together, with each [batch containing 64 packets](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/perf/src/packet.rs#L18) and forwarded to the [Signature Verification Stage](#signature-verification-stage).

## Signature Verification Stage

As we’ve seen, users must first sign the transaction before a user sends a transaction for processing. The [`SigVerifyStage`](https://github.com/solana-labs/solana/blob/638b26ea6520c3da2f0163e7530509d9442f8b12/core/src/sigverify_stage.rs#L53) is where the signatures are [verified](https://github.com/solana-labs/solana/blob/cd6f931223181d5a1d47cba64e857785a175a760/core/src/sigverify.rs#L44) before the packets are forwarded to the next stage for processing.

The node receives a list of packets and outputs the same list, but flags it telling the next stage whether the signature in that packet is valid.

## Banking Stage

The [Banking Stage](#banking-stage) decides whether to forward, hold, or process packets received. In this stage, the TPU runs each packet through the following steps:

- Deserializes the packet into transactions.
- Runs the transactions through the [Quality of Service (QoS)](./quic.md#stake-weighted-quality-of-service) model. This model prioritizes which transactions to process depending on factors such as the [transaction cost](./../transaction_fees.md) and the stake weight of the forwarding node.
- The pipeline batches the parallelizable transactions—those that don’t write to the same state simultaneously—and executes them in parallel.

At this point, the transaction’s processing is done and the leader includes the transaction on its block before forwarding the block to the next stage for broadcast to the rest of the network.

## Broadcast Stage

In the Broadcast Stage, the challenge is to get the block from the leader node communicated to the validator nodes, and in a way that doesn’t congest the network and bring throughput to a crawl. For this, Solana uses a block propagation strategy called [Turbine](./../cluster/turbine-block-propagation.md). The block is divided into small units called [shreds](./../terminology.md#shred) and each shred is broadcasted to the rest of the network via a unique path.

You can think of the network as a tree of nodes. The first node transmits the shred to a specified number of nodes in a layer below it. Each of these nodes retransmits the shred to two other nodes in the layer below them.
