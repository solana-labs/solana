---
title: 嵌入Move编程语言
---

## 面临的问题

Solana使开发人员能够使用通用编程语言（例如C或Rust）编写链上程序，但这些程序包含特定于Solana的机制。 例如，没有其他区块链要求开发人员使用`process_instruction(KeyedAccounts)`函数创建Rust模块。 如果可以，Solana应该为应用程序开发者提供更多可移植的选项。

直到最近，还没有流行的区块链提供一种可以揭示Solana大规模并行[runtime](../validator/runtime.md)价值的语言。 例如，Solidity合约不会将对共享数据的引用与合约代码分开，因此需要顺序执行以确保确定性行为。 在实践中，我们看到最积极优化的基于EVM的区块链似乎都达到了约1,200 TPS的峰值——但这只是Solana能做到的冰山一角。 另一方面，Libra项目设计了一种称为Move的链上编程语言，该语言更适合于并行执行。 像Solana的runtime一样，Move程序依赖于帐户的所有共享状态。

Solana的runtime和Libra的Move VM之间最大的设计差异是它们如何管理模块之间的安全调用。 Solana采用操作系统方法，而Libra采用特定领域语言方法。 在runtime中，模块必须捕获回到runtime中，以确保调用方的模块未写入被调用方拥有的数据。 同样，当被调用方完成操作时，它必须再次陷阱回到runtime，以确保被调用方未写入调用方拥有的数据。 另一方面，Move包含一个高级类型的系统，该系统允许其字节码验证程序运行这些检查。 由于可以验证Move字节码，因此在链上加载模块时，只需支付一次验证费用。 在runtime中，每笔交易在模块之间交叉时都要支付费用。 从本质上讲，这种区别类似于动态类型的语言（如Python）与静态类型的语言（如Java）之间的差异。 Solana的runtime允许使用通用编程语言编写应用程序，但这在程序之间跳转时会带来runtime检查的费用。

该提议尝试定义一种嵌入Move VM的方式，从而实现：

- 跨模块调用中，Move不需要runtime的跨程序

  跨程序runtime检查

- Move程序可以利用其他Solana程序中的功能，反之

  亦然

- Solana的runtime并行性暴露于批量Move和non-Move中的

  交易

## 拟定的解决方案

### 将Move VM作为Solana加载器

Move VM应该作为Solana加载器嵌入在标识符`MOVE_PROGRAM_ID`下，以便可以将Move模块标记为`可执行的`，而VM是其`所有者`。 这将允许模块加载模块依赖性，支持并行执行Move脚本。

Move模块拥有的所有数据帐户必须将其所有者设置为加载程序`MOVE_PROGRAM_ID`。 由于Move模块以与Solana程序封装其帐户相同的方式封装其帐户数据，因此Move模块所有者应嵌入在帐户数据中。 Runtime将授予对Move VM的写访问权限，而Move将授予对模块帐户的访问权限。

### 与Solana程序交互

要在非Move程序中调用指令，Solana需要通过`process_instruction()`系统调用来扩展MoveVM。 它和Rust BPF程序的`process_instruction()`方法一样。
