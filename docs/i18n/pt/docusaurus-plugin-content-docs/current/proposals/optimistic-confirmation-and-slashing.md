---
title: Optimistic Confirmation and Slashing
---

Progress on optimistic confirmation can be tracked here

https://github.com/solana-labs/solana/projects/52

At the end of May, the mainnet-beta is moving to 1.1, and testnet is moving to 1.2. With 1.2, testnet will behave as if it has optimistic finality as long as at least no more than 4.66% of the validators are acting maliciously. Applications can assume that 2/3+ votes observed in gossip confirm a block or that at least 4.66% of the network is violating the protocol.

## How does it work?

The general idea is that validators must continue voting following their last fork, unless the validator can construct a proof that their current fork may not reach finality. The way validators construct this proof is by collecting votes for all the forks excluding their own. If the set of valid votes represents over 1/3+X of the epoch stake weight, there may not be a way for the validators current fork to reach 2/3+ finality. The validator hashes the proof (creates a witness) and submits it with their vote for the alternative fork. But if 2/3+ votes for the same block, it is impossible for any of the validators to construct this proof, and therefore no validator is able to switch forks and this block will be eventually finalized.

## Tradeoffs

The safety margin is 1/3+X, where X represents the minimum amount of stake that will be slashed in case the protocol is violated. The tradeoff is that liveness is now reduced by 2X in the worst case. If more than 1/3 - 2X of the network is unavailable, the network may stall and will only resume finalizing blocks after the network recovers below 1/3 - 2X of failing nodes. So far, we haven’t observed a large unavailability hit on our mainnet, cosmos, or tezos. For our network, which is primarily composed of high availability systems, this seems unlikely. Currently, we have set the threshold percentage to 4.66%, which means that if 23.68% have failed the network may stop finalizing blocks. For our network, which is primarily composed of high availability systems a 23.68% drop in availabilty seems unlinkely. 1:10^12 odds assuming five 4.7% staked nodes with 0.995 of uptime.

## Security

Long term average votes per slot has been 670,000,000 votes / 12,000,000 slots, or 55 out of 64 voting validators. This includes missed blocks due to block producer failures. When a client sees 55/64, or ~86% confirming a block, it can expect that ~24% or `(86 - 66.666.. + 4.666..)%` of the network must be slashed for this block to fail full finalization.

## Why Solana?

This approach can be built on other networks, but the implementation complexity is significantly reduced on Solana because our votes have provable VDF-based timeouts. It’s not clear if switching proofs can be easily constructed in networks with weak assumptions about time.

## Slashing roadmap

Slashing is a hard problem, and it becomes harder when the goal of the network is to have the lowest possible latency. The tradeoffs are especially apparent when optimizing for latency. For example, ideally validators should cast and propagate their votes before the memory has been synced to disk, which means that the risk of local state corruption is much higher.

Fundamentally, our goal for slashing is to slash 100% in cases where the node is maliciously trying to violate safety rules and 0% during routine operation. How we aim to achieve that is to first implement slashing proofs without any automatic slashing whatsoever.

Neste momento, para consenso regular, após uma violação de segurança, a rede será interrompida. Nós podemos analisar os dados e descobrir quem foi responsável e propor que a estaca deve ser cortada após reiniciar. Uma abordagem semelhante será utilizada com um confuso optimista. Uma violação de segurança de conf otimista é fácil de observar, mas em circunstâncias normais, uma violação de segurança de confirmação optimista pode não parar a rede. Uma vez que a violação tenha sido observada, os validadores de congelarão a participação afetada no próximo período de tempo e decidirão sobre a próxima atualização se a violação requer bateria.

A longo prazo, as transações devem ser capazes de recuperar uma porção das garantias cortantes se a optimista violação da segurança for provada. Nesse cenário, cada bloco é efetivamente segurado pela rede.
