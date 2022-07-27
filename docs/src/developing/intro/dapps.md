---
title: What is a dApp?
description: "Decentralized applications, aka dApps, are the user facing applications of the Solana blockchain. They are written with languages like JavaScript and Rust."
keywords: ""
---

Decentralized applications, "_dApps_" for short, are the user facing applications that interact with the Solana blockchain. These applications are generally a combination of frontend code (like HTML and JavaScript) as well as [Solana Programs](./programs.md) deployed on the blockchain itself.

Since Solana Programs are "_composable_" and callable by anyone, dApp developers are not required to build and deploy their own on-chain programs. In fact, they often use a combination of custom written code for their own [on-chain programs](./programs.md#on-chain-programs), [Native programs](../runtime-facilities/programs.md), and other programs from the [Solana Program Library](https://spl.solana.com).

## dApp Examples

Decentralized apps are being used all across the Solana blockchain to accomplish different things. Some dApps are focused on decentralized finance (DeFi), empowering community governance through DAOs, or even encouraging people to get out and move their bodies.

Some of the most commonly known Solana dApps include:

- [Phantom Wallet](https://solana.com/ecosystem/phantom): Solana browser based wallet extension
- [Metaplex](https://solana.com/ecosystem/metaplex): The NFT standard on Solana
- [Squads](https://solana.com/ecosystem/squads): Collaboration and DAO tool
- [Orca](https://solana.com/ecosystem/orca): User-friendly cryptocurrency exchange built on Solana
- [StepN](https://solana.com/ecosystem/stepn): NFT game encouraging people to move their bodies
- [MagicEden](https://solana.com/ecosystem/magiceden): Poplar Solana NFT marketplace
- [Serum](https://solana.com/ecosystem/serum): High-speed, orderbook based, non-custodial DEX

You can explore more of the [Solana community dApps](https://solana.com/ecosystem) on Solana.com

## How to Build a dApp

For the frontend, user facing portion of dApps, developers can use most modern JavaScript frameworks that utilize the NPM registry for dependency management. Including React, NextJS, Vue, Node, and more.

The blockchain deployable Solana programs are commonly created with the [Rust](https://www.rust-lang.org/) programming language. But can also be created with other low level languages that can be compiled with the BPF loader like C and C++.

Due to it's developer convenience, the [Anchor framework](https://www.anchor-lang.com/) has emerged as the leading Rust based framework to develop on chain Solana programs. Anchor provides many tools and utilities for more rapidly developing Solana programs that operate on the [Sealevel runtime](https://medium.com/solana-labs/sealevel-parallel-processing-thousands-of-smart-contracts-d814b378192).

## dApp Boilerplate with NPX

The Solana developer community has created a NPM package to empower developers to rapidly scaffold the boilerplate for Solana dApps. Including wiring up the wallet adapter for the most common [Solana wallets](../../wallet-guide.md).

To quickly scaffold a Solana dApp with this tool, run the following command:

```sh
npx create-solana-dapp
```

> The [create-solana-dapp](https://www.npmjs.com/package/create-solana-dapp) package is open source and available in both the NPM registry and it's [GitHub repo](https://github.com/solana-developers/create-solana-app).
