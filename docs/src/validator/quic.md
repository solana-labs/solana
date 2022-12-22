---
title: "QUIC"
---

The Solana blockchain used the QUIC network protocol to provide both low latency connections, as well as protections against certain types of denial of service attacks.

## What is QUIC?

[QUIC](https://en.wikipedia.org/wiki/QUIC) is a transport layer network protocol that works by introducing a number of parallel streams that can transfer data simultaneously over a single connection without affecting the other streams. If the transmission fails on one stream, only that stream has to pause while the other streams continue transmission independently.

A connection is a negotiated setup between two end points similar to how a TCP connection works. Unlike TCP which requires several handshakes, a QUIC client only requires a single handshake in order to acquire the necessary information to complete the handshake. This is one of the key QUIC features that help in its low latency and establishing a quick connection.

Each connection possesses a set of connection identifiers, or connection IDs, each of which can be used to identify the connection. This is very crucial for Solana because when a sender becomes abusive by spamming, they can be more easily throttled (by tracing their connection ID) in order to control the flow of data between nodes. QUIC offers much better protection against IP spoofing and offers more congestion control methods.

Among the other advantages of [QUIC vs UDP](#quic-vs-udp) described below, QUIC allows the implementation of [stake-weighted quality of service](#stake-weighted-quality-of-service).

## Stake-weighted Quality of Service

Quality of Service (QoS) refers to the practice of prioritizing certain types of traffic when there is more traffic than the network can handle. Stake-weighted QoS means that we use stake weight as the quality to decide what traffic to prioritize. And since Solana is a proof of stake network, it is practical to use [stake](./../terminology.md#stake) as a measure of transaction quality.

RPC nodes without a stake in the network have to send transactions to staked validator nodes first, instead of sending the transactions directly to the [leader](./../terminology.md#leader). This provides a better chance of finding execution since the leader will most likely drop excess messages from non-staked nodes. This brings an end to the practice of indiscriminately accepting transactions on a first-come-first-serve basis, without regard to the source.

Because IPs are verifiable through QUIC, validators can prioritize and limit the traffic for specific connections. Instead of validators and RPCs blasting transactions at the leader as fast as they can, effectively spamming the leader, they would have a persistent QUIC connection. If the network port gets congested, it is possible to identify and throttle large traffic connections, limiting the number of messages the node can send to a leader’s port.

## QUIC vs UDP

Initially, the Solana network relied on the user datagram protocol (UDP) because it’s more efficient to the network’s use case than its common alternative, the transmission control protocol (TCP).

This is true for several reasons:

- TCP requires a three-way handshake before sending the payload. UDP doesn't require a handshake, thus reducing overhead and making it faster than TCP.

- UDP is a connectionless protocol and doesn’t guarantee the order or delivery of messages. On the other hand, if a TCP packet is lost during transmission, then the whole transmission will halt until the packet is recovered. As the packet loss rate increases, the network’s performance degrades. UDP removes potential TCP latency issues and connection management overhead while keeping server and networking setup simple.

However, it is not all milk and honey in UDP land. Without the handshake, you can't ever be certain that the source IP of the packets is genuine.

### IP Spoofing

Due to [IP spoofing](https://www.cloudflare.com/learning/ddos/glossary/ip-spoofing/), you cannot limit which IP address can send you a payload. A malicious actor can send an excessive payload to the validator’s port, leading to congestion and thus slowing down the network. The actor doesn’t even have to be malicious.

In the past, bots have spammed validators with almost 400k transactions per second (show in the graph below) while trying to outbid one another on an anticipated token sale and unwittingly executing a distributed denial of service on the network.

![Solana nodes receiving almost 400k transactions per second](/img/nodes-400k-tps-spike.png)

## Solana upgraded to QUIC

Instances like the one mentioned above brought to light Solana’s need for a communication protocol that could offer the security of TCP with the speed of UDP.

As of [v1.13.4](https://github.com/solana-labs/solana/releases/tag/v1.13.4), the Solana network upgraded to using the QUIC protocol by default.
