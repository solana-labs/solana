---
title: Transaction Fees
description: "Transaction fees are the small fees paid to process instructions on the network. These fees are based on computation and an optional prioritization fee."
keywords:
  - instruction fee
  - processing fee
  - storage fee
  - low fee blockchain
  - gas
  - gwei
  - cheap network
  - affordable blockchain
---

The small fees paid to process [instructions](./terminology.md#instruction) on the Solana blockchain are known as "_transaction fees_".

As each transaction (which contains one or more instructions) is sent through the network, it gets processed by the current leader validation-client. Once confirmed as a global state transaction, this _transaction fee_ is paid to the network to help support the [economic design](#economic-design) of the Solana blockchain.

> **NOTE:** Transaction fees are different from [account rent](./terminology.md#rent)!
> While transaction fees are paid to process instructions on the Solana network, rent is paid to store data on the blockchain.

> You can learn more about rent here: [What is rent?](./developing/intro/rent.md)

## Why pay transaction fees?

Transaction fees offer many benefits in the Solana [economic design](#basic-economic-design) described below. Mainly:

- they provide compensation to the validator network for the CPU/GPU resources necessary to process transactions,
- reduce network spam by introducing real cost to transactions,
- and provide long-term economic stability to the network through a protocol-captured minimum fee amount per transaction

> **NOTE:** Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

## Basic economic design

Many blockchain networks \(e.g. Bitcoin and Ethereum\), rely on inflationary _protocol-based rewards_ to secure the network in the short-term. Over the long-term, these networks will increasingly rely on _transaction fees_ to sustain security.

The same is true on Solana. Specifically:

- A fixed proportion (initially 50%) of each transaction fee is _burned_ (destroyed), with the remaining going to the current [leader](./terminology.md#leader) processing the transaction.
- A scheduled global inflation rate provides a source for [rewards](./implemented-proposals/staking-rewards.md) distributed to [Solana Validators](../src/running-validator.md).

### Why burn some fees?

As mentioned above, a fixed proportion of each transaction fee is _burned_ (destroyed). This is intended to cement the economic value of SOL and thus sustain the network's security. Unlike a scheme where transactions fees are completely burned, leaders are still incentivized to include as many transactions as possible in their slots.

Burnt fees can also help prevent malicious validators from censoring transactions by being considered in [fork](./terminology.md#fork) selection.

#### Example of an attack:

In the case of a [Proof of History (PoH)](./terminology.md#proof-of-history-poh) fork with a malicious, censoring leader:

- due to the fees lost from censoring, we would expect the total fees burned to be **_less than_** a comparable honest fork
- if the censoring leader is to compensate for these lost protocol fees, they would have to replace the burnt fees on their fork themselves
- thus potentially reducing the incentive to censor in the first place

## Calculating transaction fees

Transactions fees are calculated based on two main parts:

- a statically set base fee per signature, and
- the computational resources used during the transaction, measured in "[_compute units_](./terminology.md#compute-units)"

Since each transaction may require a different amount of computational resources, they are alloted a maximum number of _compute units_ per transaction known as the "[_compute budget_](./terminology.md#compute-budget)".

The execution of each instruction within a transaction consumes a different number of _compute units_. After the maximum number of _compute units_ has been consumed (aka compute budget exhaustion), the runtime will halt the transaction and return an error. This results in a failed transaction.

> **Learn more:** compute units and the [Compute Budget](./developing/programming-model/runtime#compute-budget) in the Runtime and [requesting a fee estimate](./developing/clients/jsonrpc-api.md#getfeeformessage) from the RPC.

## Prioritization fee

Recently, Solana has introduced an optional fee called the "_[prioritization fee](./terminology.md#prioritization-fee)_". This additional fee can be paid to help boost how a transaction is prioritized against others, resulting in faster transaction execution times.

The prioritization fee is calculated by multiplying the requested maximum _compute units_ by the compute-unit price (specified in increments of 0.000001 lamports per compute unit) rounded up to the nearest lamport.

You can read more about the [compute budget instruction](./developing/programming-model/runtime.md#compute-budget) here.

## Fee Collection

Transactions are required to have at least one account which has signed the transaction and is writable. Writable signer accounts are serialized first in the list of transaction accounts and the first of these accounts is always used as the "fee payer".

Before any transaction instructions are processed, the fee payer account balance will be deducted to pay for transaction fees. If the fee payer balance is not sufficient to cover transaction fees, the transaction will be dropped by the cluster. If the balance was sufficient, the fees will be deducted whether the transaction is processed successfully or not. In fact, if any of the transaction instructions return an error or violate runtime restrictions, all account changes _except_ the transaction fee deduction will be rolled back.

## Fee Distribution

Transaction fees are partially burned and the remaining fees are collected by the validator that produced the block that the corresponding transactions were included in. The transaction fee burn rate was initialized as 50% when inflation rewards were enabled at the beginning of 2021 and has not changed so far. These fees incentivize a validator to process as many transactions as possible during its slots in the leader schedule. Collected fees are deposited in the validator's account (listed in the leader schedule for the current slot) after processing all of the transactions included in a block.
