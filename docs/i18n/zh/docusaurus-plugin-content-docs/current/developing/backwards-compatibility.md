---
title: 后向兼容政策
---

随着 Solana 开发人员生态系统的发展，围绕打破 API 和行为更改（影响为 Solana 构建的应用程序和工具）的明确期望也将越来越高。 在一个完美的世界中，Solana 的开发可以以非常快的速度继续进行，而不会给现有开发人员造成任何问题。 但是，将需要做出一些妥协，因此，本文档尝试澄清和整理新发行版的过程。

### 期望值

- Solana 软件版本包括 API，SDK 和 CLI 工具(带有一些[exceptions](#exceptions))。
- Solana 软件版本遵循语义版本控制，更多详细信息请参见下文。
- 适用于`MINOR`版本的软件将与同一`MAJOR`版本上的所有软件兼容。

### 弃用过程

1. 在任何`PATCH`或`MINOR`版本中，功能、API、端点等都可以标记为已弃用。
2. 根据代码升级的难度，某些功能将在几个发布周期内被弃用。
3. 在未来的`MAJOR`版本中，不赞成使用的功能将以不兼容的方式删除。

### 发布时间

Solana RPC API、Rust SDK、CLI 工具和 BPF 程序 SDK 均已更新并随每个 Solana 软件版本一起提供，并且应始终在特定`MINOR`版本的`PATCH`更新之间兼容。

#### 发布频道

- `edge`软件，其中包含最先进的功能，没有向后兼容策略
- 在 Solana Tour de SOL 测试网集群上运行的`beta`软件
- 在 Solana Mainnet Beta 和 Devnet 集群上运行的`stable`软件

#### 主要版本(x.0.0)

`MAJOR`版本(例如 2.0.0)可能包含重大更改并删除了以前不推荐使用的功能。 客户端 SDK 和工具将开始使用在先前`MAJOR`版本中启用的新功能和端点。

#### 次要版本(1.x.0)

新功能和建议实施已添加到*new*版本的`MINOR`版本(例如 1.4.0)中，并且首先在 Solana 的 Tour de SOL 测试网集群上运行。 在测试网上运行时，`MINOR`版本被认为在`beta`发布渠道中。 在对这些更改进行了修补并证明是可靠的之后，`MINOR`版本将升级到`stable`发布渠道并部署到 Mainnet Beta 集群。

#### 修补程序版本(1.0.x)

低风险功能，不间断的更改以及安全性和错误修复作为`PATCH`版本发行版(例如 1.0.11)的一部分提供。 补丁可以同时应用于`beta`和`stable`发布渠道。

### RPC API

补丁发布：

- Bug 修复
- 安全修复
- 端点/功能弃用

次要版本：

- 新的 RPC 端点和功能

主要版本：

- 删除过时的功能

### Rust Crates

- [`solana-sdk`](https://docs.rs/solana-sdk/) - 用于创建交易和解析帐户状态的 Rust SDK
- [`solana-program`](https://docs.rs/solana-program/) - 用于编写程序的 Rust SDK
- [`solana-client`](https://docs.rs/solana-client/) - 用于连接 RPC API 的 Rust 客户端
- [`solana-cli-config`](https://docs.rs/solana-cli-config/) - 用于管理 Solana CLI 配置文件的 Rust 客户端

补丁发布：

- Bug 修复
- 安全修复
- 性能提升

次要版本：

- 新的 API

主要版本：

- 删除过时的 API
- 向后不兼容的行为更改

### CLI 工具

补丁发布：

- 错误和安全修复
- 性能提升
- 弃用子命令/参数

次要版本：

- 新的子命令

主要版本：

- 切换到先前主要版本中引入的新 RPC API 端点/配置。
- 删除过时的功能

### Runtime 功能

Solana 新的 runtime 功能已进行功能切换并手动激活。 Runtime 功能包括：引入新的本机程序，sysvars 和 syscalls；并改变他们的行为。 功能激活与群集无关，因此可以在 Mainnet-beta 上激活之前在 Testnet 上建立置信度。

发布过程如下：

1. 新版本中包含新的 runtime 功能，默认情况下已禁用
2. 一旦有足够的抵押验证程序升级到新版本，就可以通过指令手动激活 runtime 功能开关
3. 该功能在下一个纪元开始时生效

### 基础架构变更

#### 公共 API 节点

Solana 提供了公开可用的 RPC API 节点，供所有开发人员使用。 Solana 团队将尽最大努力将任何更改传达给主机，端口，限速行为，可用性等。 但是，我们建议开发人员依靠自己的验证器节点来阻止对 Solana 操作的节点的依赖。

#### 本地集群脚本和 Docker 映像

重大更改将仅限于`MAJOR`版本更新。 `MINOR`和`PATCH`更新应始终向后兼容。

### 例外

#### Web3 JavaScript SDK

Web3.JS SDK 还遵循语义版本控制规范，但与 Solana 软件版本分开提供。

#### 攻击向量

如果在现有代码中发现了新的攻击媒介，则可以根据问题的严重性来规避上述过程，以便快速部署修复程序。
