---
title: 测试程序
---

应用程序将交易发送到Solana集群和查询验证者，以确认交易被处理，并检查每个交易的结果。 当集群的行为与预期不符时，可能有多种原因。

- 程序有问题
- BPF加载器拒绝了一条不安全的程序指令。
- 这笔交易太大
- 交易无效
- 运行时试图执行交易时，另一个人正在访问

  同一个账户

- 网络放弃了交易
- 集群回滚账目
- 一个验证者恶意回应查询

## AsyncClient和SyncClient特征

为了排除故障，应用程序应该重新瞄准一个较低级的组件，因为在那里可能出现的错误较少。 重定向可以通过AsyncClient和SyncClient特性的不同实现来完成。

组件实现了以下主要方法。

```text
trait AsyncClient {
    fn async_send_transaction(&self, transaction: Transaction) -> io::Result<Signature>;
}

trait SyncClient {
    fn get_signature_status(&self, signature: &Signature) -> Result<Option<transaction::Result<()>>>;
}
```

用户发送交易并异步和同步等待结果。

### 集群的轻量级客户端

最高级别的实现，ThinClient，以Solana集群为目标，它可能是一个已部署的测试网，也可能是一个在开发机器上运行的本地集群。

### 针对 TPU 的 TpuClient

下一个层次是TPU的实现，目前还没有实现。 在TPU层面，应用程序通过Rust通道发送交易，在这个通道中，不会出现来自网络队列或丢包的意外。 TPU实现了所有 "正常 "的交易错误。 它进行签名验证，可能会报告账户使用中的错误，否则就会在分类账中产生结果，并完成历史哈希证明。

## 低级测试

### 银行的银行客户端

TPU下面是银行。 银行不进行签名验证，也不生成账本。 银行是一个方便的层，可以测试新的链上程序。 它允许开发者在本地程序实现和 BPF 编译的变体之间切换。 这里不需要Transact特性。 银行的API是同步的。

## 使用runtime进行单元测试

Bank下面是Runtime。 Runtime是单元测试的理想测试环境。 通过将Runtime静态地链接到本地程序实现中，开发者可以获得尽可能短的编辑-编译-运行循环。 在没有任何动态链接的情况下，堆栈痕迹包括调试符号，程序错误也很容易排除。
