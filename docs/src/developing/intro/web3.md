---
title: What is Web3?
description: ""
keywords: "web3 meaning, web3.js, web3js, web3 vs web2, npm, yarn, pnpm, JavaScript, web3 explained"
---

Web3 is the next phase of the internet, focused on being built on top of decentralized technologies like the Solana blockchain. It is an idea that people will no longer be required to rely on large organizations to act as a trusted intermediaries to interact with one another. Instead, web3 can rely on public, secure, and auditable blockchains to act as trusted sources of information and interactions.

Since the concept of web3 is still relatively new, many people have different specific definitions of what web3 _can_ and _should_ be. But most people agree upon a common theme: 

- people to have more privacy on the internet
- people being able to monetize their own efforts and/or content on the internet, in ways they choose

## Web2 vs Web3

In the traditional web2 world, centralized organizations are at the center of the internet and the content being created online (including monetizing that content). In general, people are creating the content being consumed online, and the web2 organizations are controlling that information. Generating various amounts of profits from their user generated content. Sometimes giving small fractions of monetary rewards to their creators. But often at the expense of their user's privacy.

In a web3 world, users would be able to create their own content online. They can choose how their personal information and content is handled by other individuals or organizations. From written articles or blog posts to custom works of art, the content creators would be able to choose their preferred distribution method. They can choose what of their personal information is attached to their online creations.  

In the end, web3 is about giving **choice** back to individuals.

## What can be built with web3?

People and organizations are constantly building new applications and experiences on web3 technology.

A few examples of the types of applications, organizations, and ideas being built in web3 include:

- [Programmable Blockchains](https://solana.com)
- [Decentralized Autonomous Organizations (DAOs)](#daos)
- [Decentralized Finance (DeFi)](#defi)
- [Non-Fungible Tokens (NFTs)](#nfts)

Almost anything can and is being built with web3/blockchain technologies, and the possibilities are near endless.

#### DAOs

Decentralized Autonomous Organizations, or _DAOs_ for short, allows people from around to world to collaborate together for a common goal. They can be developing protocols and sharing source code or raising funds for charitable causes. Or even operate a social club of sorts that just aims to talk about a common topic.

These organizations can decide internally how to govern themselves or make progress on their goals. DAOs can be structured similar to how a publicly traded company functions, with a head boss and a core team, or even operate in a "_flat structure_" where every individual's vote is worth the same. DAOs can choose to operate however they choose.

#### DeFi

Decentralized Finance, _DeFi_ for short, is the term used to describe a new industry of financial services and protocols that empower people to send, receive, exchange, or distribute value without the need for centralized organizations like banks. 

DeFi empowers people to send money to anyone around the world in a cheap, fast, and auditable way. People can earn interest on DeFi investments or loans.

#### NFTs

Non-Fungible Tokens, _NFTs_ for short, are unique blockchain based tokens or assets. These tokens use public blockchains to prove ownership over digital items. 

NFTs are being used for many purposes. From selling unique works of art to concert tickets and access tokens for exclusive events or groups.

## Web3 developers

Every day, new developers are becoming interested and joining the web3 ecosystem. Each learning and building different things. With each application being built, web3 developers will need to make careful considerations in the technology and tech stack they choose.

Some major considerations for building in web3 are **cost**, **speed**, and **programmability**. Each blockchain has it's own benefits and drawbacks for building software and [dApps](../intro/dapps.md)

The Solana blockchain is one of the very few blockchains that meets all of these considerations. 

### Languages and frameworks

In general, Solana web3 developers use a few languages and frameworks to develop on Solana.

Like most modern applications, Solana web3 developers can be broadly categorized into 3 main categories:

- [frontend](#frontend) - often with JavaScript/Typescript based frameworks like React, NextJS, and Vue
- [backend](#backend) (aka on-chain) - low level languages like Rust and C++
- fullstack (aka both frontend and backend)

## Backend

[Solana programs](../intro/programs.md), sometimes referred to as _smart contracts_ on other blockchains, are the on-chain exectuable code that runs in the Solana runtime. 

The development of these [on-chain programs](../on-chain-programs/overview.md) is similar to the development of the _backend_ code for a traditional application or service. As such, on-chain programs are usually writting with the following languages/frameworks:

- [Rust](../on-chain-programs/developing-rust.md) - most common language to write programs
- [Anchor](hhttps://anchor-lang.com/) - popular Rust based framework for Solana programs
- [C/C++](../on-chain-programs/developing-c.md) - less common languages to write programs

## Frontend

Similar to most modern applications, JavaScript is used on the user-facing _frontend_ of web3 applications and [dApps](../intro/dapps.md). And just like other modern web apps, web3 applications can often use many of the popular JavaScript based frameworks, like:

- React
- NextJS
- VueJS
- and more

> **Get started quickly:** You can use the [create-solana-dapp](https://www.npmjs.com/package/create-solana-dapp) NPM package to quickly scaffold a full stack Solana application. Selecting you desired framework/language for both the frontend and backend.

### Web3.js

The `@solana/web3.js` library is a JavaScript package that provides a standard way to interact with the [Solana JSON RPC](#) methods. Enabling JavaScript based applications and [dApps](./dapps.md) to easily make JSON RPC calls using common JavaScript conventions like `import` and/or `require` statements.

### Install web3.js

The `@solana/web3.js` package is open source and can be installed via the official [NPM repository](https://www.npmjs.com/package/@solana/web3.js) (using your desired package manager like NPM, Yarn, PNPM, etc):

```bash
npm i @solana/web3.js
```

### Web3.js documentation

The full documentation for the `@solana/web3.js` package is available here: [https://solana-labs.github.io/solana-web3.js/](https://solana-labs.github.io/solana-web3.js/)

### Web3.js tutorial

You can find a full introduction [tutorial for web3.js](../clients/javascript-reference.md) here on the Solana documentation.

The [Solana Cookbook](https://solanacookbook.com) also has extensive task based documentation on using the web3.js library.