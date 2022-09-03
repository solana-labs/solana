---
title: Transaction Fees
description: "Transaction fees are the small fees paid to process instructions on the network. These fees are based on computation and an optional prioritization fee."
keywords: "instruction fee, processing fee, storage fee, low fee blockchain, gas, gwei, cheap network, affordable blockchain"
---

The small fees paid to process [instructions](./terminology.md#instruction) on the Solana blockchain are known as "_transaction fees_".

As each transaction (which contains one or more instructions) is sent through the network, it gets processed by the current leader validation-client. Once confirmed as a global state transaction, this _transaction fee_ is paid to the network to help support the [economic design](#economic-design) of the Solana blockchain.

> **NOTE:** Transaction fees are different from [account rent](./terminology.md#rent)!
> While transaction fees are paid to process instructions on the Solana network, rent is paid to store data on the blockchain.

<!-- > You can learn more about rent here: [What is rent?](./developing/intro/rent.md) -->

## Why pay transaction fees?

Transaction fees offer many benefits in the Solana [economic design](#basic-economic-design) described below. Mainly:

- they provide compensation to the validator network for the CPU/GPU resources necessary to process transactions,
- reduce network spam by introducing real cost to transactions,
- and provide potential long-term economic stability of the network through a protocol-captured minimum fee amount per transaction

> **NOTE:** Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

## Basic economic design

Many current blockchain economies \(e.g. Bitcoin, Ethereum\), rely on _protocol-based rewards_ to support the economy in the short term. And when the protocol derived rewards expire, predict that the revenue generated through _transaction fees_ will support the economy in the long term.

In an attempt to create a sustainable economy on Solana through _protocol-based rewards_ and _transaction fees_:

- a fixed portion (initially 50%) of each transaction fee is _burned_ (aka destroyed),
- with the remaining fee going to the current [leader](./terminology.md#leader) processing the transaction.

A scheduled global inflation rate provides a source for [rewards](./implemented-proposals/staking-rewards.md) distributed to [Solana Validators](../src/running-validator.md).

### Why burn some fees?

As mentioned above, a fixed proportion of each transaction fee is _burned_ (aka destroyed). The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time. While still providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

Burnt fees can also help prevent malicious validators from censoring transactions by being considered in [fork](./terminology.md#fork) selection.

#### Example of an attack:

In the case of a [Proof of History (PoH)](./terminology.md#proof-of-history-poh) fork with a malicious, censoring leader:

- due to the fees lost from censoring, we would expect the total fees destroyed to be **_less than_** a comparable honest fork
- if the censoring leader is to compensate for these lost protocol fees, they would have to replace the burnt fees on their fork themselves
- thus potentially reducing the incentive to censor in the first place

## Calculating transaction fees

Transactions fees are calculated based on two main parts:

- a statically set base fee per signature, and
- the computational resources used during the transaction, measured in "[_compute units_](./terminology.md#compute-units)"

Since each transaction may require a different amount of computational resources, they are alloted a maximum number of _compute units_ per transaction known as the "[_compute budget_](./terminology.md#compute-budget)".

The execution of each instruction within a transactions consumes a different number of _compute units_. After the maximum number of _computer units_ has been consumed (aka compute budget exhaustion), the runtime will halt the transaction and return an error. Resulting in a failed transaction.

> **Learn more:** compute units and the [Compute Budget](./developing/programming-model/runtime#compute-budget) in the Runtime and [requesting a fee estimate](./developing/clients/jsonrpc-api.md#getfeeformessage) from the RPC.

## Prioritization fee

Recently, Solana has introduced an optional fee called the "_[prioritization fee](./terminology.md#prioritization-fee)_". This additional fee can be paid to help boost how a transaction is prioritized against others, resulting in faster transaction execution times.

The prioritization fee is calculated by multiplying the requested maximum _compute units_ by the compute-unit price (specified in increments of 0.000001 lamports per compute unit) rounded up to the nearest lamport.

You can read more about the [compute budget instruction](./developing/programming-model/runtime.md#compute-budget) here.
