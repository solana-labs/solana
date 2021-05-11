---
title: "概述"
---

一个[app](terminology.md#app)通过向Solana集群发送带有一个或多个[指令](transactions.md#instructions)的[事务](transactions.md)来与之交互。 Solana [runtime](runtime.md) 会将这些指令传递给应用程序开发人员事先部署的[程序](terminology.md#program)。 例如，一条指令可能告诉程序将[lamports](terminology.md#lamports)从一个[账户](accounts.md)转移到另一个，或创建一个交互协议来管理Lamport的转移方式。 指令针对每个交易顺序地和原子级地执行。 如果任何指令无效，则交易中的所有帐户更改都将被丢弃。

要立即开始开发，您可以构建，部署和运行[示例](developing/on-chain-programs/examples.md)之一。