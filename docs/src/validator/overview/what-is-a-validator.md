---
title: What is a Validator?
---

A validator is a computer that helps to run the Solana network. Each validator executes a program that keeps track of all accounts on the Solana cluster and validates transactions being added to the network. Without validators, Solana would not be able to function.

The more independent entities that run validators, the less vulnerable the cluster is to an attack or catastrophe that affects the cluster.

> For an more in depth look at the health of the Solana network, see the [Solana Foundation Validator Health Report](https://solana.com/news/validator-health-report-march-2023).

By becoming a validator, you are helping to grow the network. You are also learning first hand how the Solana cluster functions at the lowest level. You will become part of an active community of operators that are passionate about the Solana ecosystem.

## Consensus vs RPC

Before, we discuss validators in more detail, it's useful to make some distinctions. Using the same validator software, you have the option of running a voting/consensus node or choosing to instead run an RPC node. An RPC node helps Solana devs and others interact with the blockchain but for performance reasons should not vote. We go into more detail on RPC nodes in the next section, [what is an rpc node](./what-is-an-rpc-node.md).

For this document, when a validator is mentioned, we are talking about a voting/consensus node. Now, to better understand what your validator is doing, it would help to understand how the Solana network functions in more depth.

## Proof Of Stake

Proof of stake is the blockchain architecture that is used in Solana. It is called proof of stake because token holders can stake their tokens to a validator of their choice. When a person stakes their tokens, that person still owns the tokens and can remove the stake at any time. The staked tokens represent their trust in that validator. When a person stakes their tokens to a validator, they are given a return of some amount of tokens as a reward for helping to run and secure the network. The more tokens you have staked to a validator the more rewards you receive. A validator that has a large amount of tokens staked to it has a larger vote share in consensus. A validator, therefore, is given more opportunities to produce blocks in the network proportional to the size of the stake in the validator. The validator that is currently producing blocks in the network is known as the leader.

## Proof Of Work: For Contrast

Solana is not a proof of work system. Proof of work is a different blockchain architecture in which a computer (often called a miner), works to solve a cryptographic problem before anyone else on the network is able to solve it. The more often the computer solves these problems, the more rewards the miner receives. Because of the incentive to solve a hard computational problem first, miners often use many computers at the same time. The number of computers used to solve these problems leads to large energy consumption and resulting environmental challenges.

Solana, in contrast, does not incentivize validators to use many computers to solve a computational problem. Because a validator would like to have a larger amount staked to it, there is no real advantage for an independent validator to using many different computers. Here, you can see a comparison of [Solana's environmental impact](https://solana.com/news/solana-energy-usage-report-november-2021).

## Proof Of History

Proof of history, PoH, is one of the key innovations in Solana that allows transactions to be finalized very quickly. At a high level, PoH allows validators in the cluster to agree on a cryptographically repeatable clock. Both proof of stake and proof of work architectures mentioned above are architectures that bring the cluster to consensus. In other words, these algorithms decide which blocks should be added to the blockchain. Proof of history is not a consensus architecture, but rather a feature in Solana that makes block finalization faster in the proof of stake system.

Understanding how PoH works is not necessary to run a good validator, but a very approachable discussion can be found [in this Medium article](https://medium.com/solana-labs/proof-of-history-explained-by-a-water-clock-e682183417b8). Also, the [Solana whitepaper](https://solana.com/solana-whitepaper.pdf) does a good job of explaining the algorithm in an approachable way for the more technically minded.

## Your Role As A Validator

As a validator, you are helping to secure the network by producing and voting on blocks and to improve decentralization by running an independent node. You have the right to participate in discussions of changes on the network. You are also assuming a responsibility to keep your system running properly, to make sure your system is secure, and to keep it up to date with the latest software. As more individuals stake their tokens to your validator, you can reward their trust by running a high performing and reliable validator. Hopefully, your validator is performing well a majority of the time, but you should also have systems in place to respond to an outage at any time of the day. If your validator is not responding late at night, someone (either you or other team members) need to be available to investigate and fix the issues.

Running a validator is a [technical and important task](./validator-prerequisites.md), but it can also be very rewarding. Good luck and welcome to the community.