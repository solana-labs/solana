---
title: Embedding the Move Language
---

## 面临的问题

Solana 使开发人员能够使用通用编程语言（例如 C 或 Rust）编写链上程序，但这些程序包含特定于 Solana 的机制。 例如，没有其他区块链要求开发人员使用`process_instruction(KeyedAccounts)`函数创建 Rust 模块。 如果可以，Solana 应该为应用程序开发者提供更多可移植的选项。

直到最近，还没有流行的区块链提供一种可以揭示 Solana 大规模并行[runtime](../validator/runtime.md)价值的语言。 例如，Solidity 合约不会将对共享数据的引用与合约代码分开，因此需要顺序执行以确保确定性行为。 在实践中，我们看到最积极优化的基于 EVM 的区块链似乎都达到了约 1,200 TPS 的峰值——但这只是 Solana 能做到的冰山一角。 另一方面，Libra 项目设计了一种称为 Move 的链上编程语言，该语言更适合于并行执行。 像 Solana 的 runtime 一样，Move 程序依赖于帐户的所有共享状态。

Solana 的 runtime 和 Libra 的 Move VM 之间最大的设计差异是它们如何管理模块之间的安全调用。 Solana 采用操作系统方法，而 Libra 采用特定领域语言方法。 在 runtime 中，模块必须捕获回到 runtime 中，以确保调用方的模块未写入被调用方拥有的数据。 同样，当被调用方完成操作时，它必须再次陷阱回到 runtime，以确保被调用方未写入调用方拥有的数据。 另一方面，Move 包含一个高级类型的系统，该系统允许其字节码验证程序运行这些检查。 由于可以验证 Move 字节码，因此在链上加载模块时，只需支付一次验证费用。 在 runtime 中，每笔交易在模块之间交叉时都要支付费用。 从本质上讲，这种区别类似于动态类型的语言（如 Python）与静态类型的语言（如 Java）之间的差异。 Solana 的 runtime 允许使用通用编程语言编写应用程序，但这在程序之间跳转时会带来 runtime 检查的费用。

该提议尝试定义一种嵌入 Move VM 的方式，从而实现：

- 跨模块调用中，Move 不需要 runtime 的跨程序

  跨程序 runtime 检查

- Move 程序可以利用其他 Solana 程序中的功能，反之

  亦然

- Solana 的 runtime 并行性暴露于批量 Move 和 non-Move 中的

  交易

## 拟定的解决方案

### 将 Move VM 作为 Solana 加载器

Move VM 应该作为 Solana 加载器嵌入在标识符`MOVE_PROGRAM_ID`下，以便可以将 Move 模块标记为`可执行的`，而 VM 是其`所有者`。 这将允许模块加载模块依赖性，支持并行执行 Move 脚本。

Move 模块拥有的所有数据帐户必须将其所有者设置为加载程序`MOVE_PROGRAM_ID`。 由于 Move 模块以与 Solana 程序封装其帐户相同的方式封装其帐户数据，因此 Move 模块所有者应嵌入在帐户数据中。 Runtime 将授予对 Move VM 的写访问权限，而 Move 将授予对模块帐户的访问权限。

### 与 Solana 程序交互

要在非 Move 程序中调用指令，Solana 需要通过`process_instruction()`系统调用来扩展 MoveVM。 它和 Rust BPF 程序的`process_instruction()`方法一样。
