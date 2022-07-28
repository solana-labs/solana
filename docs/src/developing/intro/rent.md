---
title: What is rent?
description: "Rent: the small fee Solana accounts incur to store data on the blockchain. Accounts with >2 years of rent are rent exempt and do not pay the periodic fee."
keywords: ""
---

The small fee every Solana Account (or Program) to store data on the blockchain is called "*rent*". This *time and space* based fee is required to keep an account, and its therfore its data, alive on the blockchain since [clusters](../../cluster/overview.md) must actively maintain this data.

Rent can be "*collected*" in two primary ways:

- when certain [specified conditions](#collecting-rent) occur that interact with the specific data in an account 
- or, not collected at all when an account is [rent exempt](#rent-exempt)

When an Account no longer has enough LAMPORTS to pay its rent, it will be removed from the network in a process known as [Garbage Collection](#garbage-collection).

> **Note:** Rent is different from [transactions fees](../../transaction_fees.md). Rent is paid (or held in an Account) to keep data stored on the Solana  blockchain. Where as transaction fees are paid to process [instructions](../developing/../programming-model/transactions.md#instructions) on the network.

### Rent rate

The Solana rent rate is set on a network wide basis, primarily based on the set LAMPORTS *per* byte *per* year.

Currently, the rent rate is a static amount and stored in the the [Rent sysvar](../runtime-facilities/sysvars.md#rent).

## Rent exempt

Accounts that maintain a minimum LAMPORT balance greater than 2 years worth of rent payments are considered "*rent exempt*" and will not incur a rent collection. 

> At the time of writing this, new Accounts and Programs **are required** to be initialized with enough LAMPORTS to become rent-exempt. The RPC endpoints have the ability to calculate this [estimated rent exempt balance](../clients/jsonrpc-api.md#getminimumbalanceforrentexemption) and is recommended to be used.

Every time an account's balance is reduced, a check is performed to see if the account is still rent exempt. Transactions that would cause an account's balance to drop below the rent exempt threshold will fail.

## Collecting rent

Accounts that are not rent exempt will have to pay their rent in the two following cases:

1. when referenced by a transaction, and
2. at least once an [epoch](../../terminology.md#epoch)

### Referenced by a transaction

This type of rent collection included the transaction that initially creates the account, as well as during the normal transaction processing by the bank.

### Once an epoch

At least once an epoch, rent is collected from Solana Accounts that are not rent exempt. This is to ensure that even stale accounts (aka accounts that were not referenced during the recent epoch) have rent collected.

This type of rent collection requires a scan of the all accounts and is spread over the entire epoch. This is to help avoid load spikes on the network.

## Garbage collection

Accounts that do not maintain their rent exempt status, or have a balance high enough to pay rent, are removed from the network in a process known as *garbage collection*. This process is done to help reduce the network wide storage of no longer used/maintained data.

You can learn more about [garbage collection here](../../implemented-proposals/persistent-account-storage.md#garbage-collection) in this implemented proposal.

## Learn more about Rent

You can learn more about Solana Rent with the following articles and documentation:

- [Implemented Proposals - Rent](../../implemented-proposals/rent.md)
- [Implemented Proposals - Account Storage](../../implemented-proposals/persistent-account-storage.md)