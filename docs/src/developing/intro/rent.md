---
title: What is rent?
description: "Rent: the small fee Solana accounts incur to store data on the blockchain. Accounts with >2 years of rent are rent exempt and do not pay the periodic fee."
# keywords:
# -
---

The fee every Solana Account to store data on the blockchain is called "_rent_". This _time and space_ based fee is required to keep an account, and its therefore its data, alive on the blockchain since [clusters](../../cluster/overview.md) must actively maintain this data.

All Solana Accounts (and therefore Programs) are required to maintain a high enough LAMPORT balance to become [rent exempt](#rent-exempt) and remain on the Solana blockchain.

When an Account no longer has enough LAMPORTS to pay its rent, it will be removed from the network in a process known as [Garbage Collection](#garbage-collection).

> **Note:** Rent is different from [transactions fees](../../transaction_fees.md). Rent is paid (or held in an Account) to keep data stored on the Solana blockchain. Where as transaction fees are paid to process [instructions](../developing/../programming-model/transactions.md#instructions) on the network.

### Rent rate

The Solana rent rate is set on a network wide basis, primarily based on the set LAMPORTS _per_ byte _per_ year.

Currently, the rent rate is a static amount and stored in the the [Rent sysvar](../runtime-facilities/sysvars.md#rent).

## Rent exempt

Accounts that maintain a minimum LAMPORT balance greater than 2 years worth of rent payments are considered "_rent exempt_" and will not incur a rent collection.

> At the time of writing this, new Accounts and Programs **are required** to be initialized with enough LAMPORTS to become rent-exempt. The RPC endpoints have the ability to calculate this [estimated rent exempt balance](/api#getminimumbalanceforrentexemption) and is recommended to be used.

Every time an account's balance is reduced, a check is performed to see if the account is still rent exempt. Transactions that would cause an account's balance to drop below the rent exempt threshold will fail.

## Garbage collection

Accounts that do not maintain their rent exempt status, or have a balance high enough to pay rent, are removed from the network in a process known as _garbage collection_. This process is done to help reduce the network wide storage of no longer used/maintained data.

You can learn more about [garbage collection here](../../implemented-proposals/persistent-account-storage.md#garbage-collection) in this implemented proposal.

## Learn more about Rent

You can learn more about Solana Rent with the following articles and documentation:

- [Implemented Proposals - Rent](../../implemented-proposals/rent.md)
- [Implemented Proposals - Account Storage](../../implemented-proposals/persistent-account-storage.md)
