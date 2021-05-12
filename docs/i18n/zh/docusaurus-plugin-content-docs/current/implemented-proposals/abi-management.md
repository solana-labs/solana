---
title: Solana ABI管理流程
---

本文件提出了Solana ABI管理流程。 ABI管理流程是一种工程实践和一个支持性的技术框架，以避免引入意外的不兼容的ABI变化。

# 面临的问题

Solana ABI(集群的二进制接口) 目前仅由实现隐式定义，需要非常仔细的观察才能注意到破坏性的变化。 这使得在不重启账本的情况下，在现有集群上升级软件非常困难。

# 需求和目标

- 意外的ABI变化可以被机械地检测为CI故障
- 新的实现必须能够处理最老的数据(世纪以来)，一旦我们进入主网。
- 这个建议的目的是保护ABI，同时通过选择机械过程而不是非常长的人为驱动的审计过程来维持相当快速的发展。
- 一旦经过加密签名，数据blob必须是相同的，所以无论在线系统的入站和出站，都不可能进行原地数据格式更新。 另外，考虑到我们要处理的交易量，最好不要进行追溯性的就地更新。

# 解决方案

我们需要一个系统性的保证，在修改源码时不破坏集群，而不是自然的人为尽职，因为人为尽职被假定为经常发生故障。

为此，我们引入了一种机制，在源代码中对每一个与ABI相关的事物(`struct`s, `enum`s) 用新的`#[frozen_abi]`属性进行标记。 它通过`ser::Serialize`从其字段的类型中提取硬编码的摘要值。 而且该属性会自动生成一个单元测试，试图检测任何未经批准的对标记的ABI相关事物的更改。

但是，检测不可能是完全的，无论我们如何努力静态分析源代码，仍然有可能破坏ABI。 例如，这包括非`派生`的手写`ser::Serialize`、底层库的实现变化(例如`bincode`)、CPU架构差异。 对这些可能的ABI不兼容的检测不在这个ABI管理的范围之内。

# 定义

ABI项目/类型：用于序列化的各种类型，共同构成任何系统组件的整个ABI。 例如，这些类型包括`struct`s和`enum`s。

ABI项目摘要。从ABI项的字段的类型信息导出的一些固定的哈希值。

# 示例

```patch
+#[frozen_abi(digest="eXSMM7b89VY72V...")]
 #[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
 pub struct Vote {
     /// A stack of votes starting with the oldest vote
     pub slots: Vec<Slot>,
     /// signature of the bank's state at the last slot
     pub hash: Hash,
 }
```

# 开发者的工作流程

为了知道新的ABI项目的摘要，开发人员可以用一个随机的摘要值添加`frozen_abi`，然后运行单元测试，并从断言测试错误信息中用正确的摘要来替换它。

一般来说，一旦我们添加了`frozen_abi`，并且它的变化被发布在稳定版本频道中，它的摘要应该永远不会改变。 如果需要这样的改变，我们应该选择定义一个新的`struct`，比如`FooV1`。 而特殊的发布流程，比如硬分叉，则应该类似。

# 实施说明

我们使用某种程度的宏机制来自动生成单元测试，并从ABI项目中计算出一个摘要。 通过巧妙地使用`serde::Serialize`(`[1]`)和`any::type_name`(`[2]`)可以实现这一点。 对于类似的实现先例，Parity Technologies`[3]`中的`ink`可以作为参考。

# 实现细节

本实施方案的目标是尽可能自动检测ABI的意外变化。 为此，结构性ABI信息的摘要是以最大努力的准确性和稳定性计算的。

当ABI摘要检查运行时，通过重复使用`serde`的序列化功能、过程宏和通用特殊化功能，对ABI项的字段的ABI进行递归摘要，动态计算出ABI摘要。 然后，检查`assert!`其最终的摘要值与`frozen_abi`属性中指定的相同。

为了实现这一点，它创建了一个该类型的实例和一个自定义的`Serializer`实例，为`serde`递归遍历它的字段，就像真正的序列化实例一样。 这种遍历必须通过`serde`来完成，才能真正捕捉到什么样的数据实际上会被`serde`序列化，即使考虑到定制的非`派生`的`Serialize`接口实现。

# ABI摘要过程

这一部分有点复杂。 有三个相互依赖的部分：`AbiExample`、`AbiDigester`和`AbiEnumVisitor`。

首先，生成的测试会创建一个摘要类型的实例，这个实例有一个叫做`AbiExample`的接口，它应该像`Serialize`一样为所有的摘要类型实现，并像`Default`接口一样返回`Self`。 通常情况下，它是通过通用的接口特殊化来提供给大多数常见类型的。 也可以为`struct`和`enum`进行`派生`，如果需要，也可以手工编写。

自定义的`serializer`被称为`AbiDigester`。 而当它被`serde`调用来序列化一些数据时，它会尽可能地递归收集ABI信息。 `AbiDigester`根据数据类型的不同，对ABI摘要的内部状态进行不同的更新。 这个逻辑是通过一个名为`AbiEnumVisitor`的接口为每个`enum`类型专门重定向的。 顾名思义，没有必要为其他类型实现`AbiEnumVisitor`。

总结一下这种相互作用，`serde`与`AbiDigester`串联处理递归序列化控制流。 测试中的初始入口点和子`AbiDigester`使用`AbiExample`递归地创建一个示例对象层次图。 而`AbiDigester`使用`AbiEnumVisitor`使用构建的样本查询实际的ABI信息。

`Default`对于`AbiExample`来说是不够的。 多种集合的`::default()`是空的，但我们想用实际的项目来摘要它们。 而且，ABI摘要不能只用`AbiEnumVisitor`来实现。 需要`AbiExample`是因为需要一个实际的类型实例来通过`serde`实际遍历数据。

另一方面，ABI摘要也不能只用`AbiExample`来完成。 需要`AbiEnumVisitor`，因为一个`enum`的所有变体不能只用它的一个变体作为ABI实例来遍历。

可摘要的信息：

- rust的类型名称
- `serde`的数据类型名称。
- `struct`中的所有字段
- `enum`中的所有变体。
- `struct`：正常(`struct {...}`) 和元组式(`struct(...)`)
- `enum`：正常变体和`struct`-和`tuple`-风格。
- 属性：`serde(serialize_with=...)`和`serde(skip)`

不可摘要的信息：

- `AbiExample`提供的样本未触及的任何自定义序列化代码路径。 (技术上不可能)
- 属(必须是具体类型；对具体类型别名使用`frozen_abi`)

# 参考文献

1. [(De)Serialization with type info · Issue #1095 · serde-rs/serde](https://github.com/serde-rs/serde/issues/1095#issuecomment-345483479)
2. [`std::any::type_name` - Rust](https://doc.rust-lang.org/std/any/fn.type_name.html)
3. [Parity's ink to write smart contracts](https://github.com/paritytech/ink)
