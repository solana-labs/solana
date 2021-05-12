---
title: "개요"
---

[앱](terminology.md#app)은 다수의 [명령](transactions.md#instructions)을 담은 [트랜잭션](transactions.md)을 솔라나 클러스터로 보내면서 상호작용 합니다. The Solana [runtime](runtime.md) passes those instructions to [programs](terminology.md#program) deployed by app developers beforehand. An instruction might, for example, tell a program to transfer [lamports](terminology.md#lamport) from one [account](accounts.md) to another or create an interactive contract that governs how lamports are transferred. Instructions are executed sequentially and atomically for each transaction. If any instruction is invalid, all account changes in the transaction are discarded.

To start developing immediately you can build, deploy, and run one of the [examples](developing/on-chain-programs/examples.md).
