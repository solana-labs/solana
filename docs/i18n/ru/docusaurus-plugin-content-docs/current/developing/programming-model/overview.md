---
title: "Overview"
---

An [app](terminology.md#app) interacts with a Solana cluster by sending it
[transactions](transactions.md) with one or more
[instructions](transactions.md#instructions). The Solana [runtime](runtime.md)
passes those instructions to [programs](terminology.md#program) deployed by app
developers beforehand. An instruction might, for example, tell a program to
transfer [lamports](terminology.md#lamport) from one [account](accounts.md) to
another or create an interactive contract that governs how lamports are
transferred. Instructions are executed sequentially and atomically for each
transaction. If any instruction is invalid, all account changes in the
transaction are discarded.

To start developing immediately you can build, deploy, and run one of the
[examples](developing/on-chain-programs/examples.md).
