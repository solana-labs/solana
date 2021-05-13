---
title: 集群基准
---

Solana git 仓库涵盖了配置本地测试网可能用到的所有脚本。 根据实现的目标，您可能想配置一个全新、增强性能的不同版本的多节点测试网，那么它可能比单纯的仅支持 Rust 的单节点测试节点要复杂得多。 如果您正在尝试开发高级功能（例如智能合约），那么利用一些已有的配置，直接使用 Rust 支持的单节点模型就好。 如果您正在对交易流程进行性能优化，请考虑增强的单节点 demo。 如果你在着手共识算法的工作，那么你将至少需要一个 Rust 的多节点 demo。 如果您想要复制 TPS 性能表，请运行强化的多节点 demo。

对于上述的四种变型，您可能需要最新的 Rust 工具链和 Solana 源代码：

首先，请设置 Solana [README](https://github.com/solana-labs/solana#1-install-rustc-cargo-and-rustfmt) 中提到的 Rust、Cargo 和系统安装包。

请检查 github 代码：

```bash
git clone https://github.com/solana-labs/solana.git
cd solana
```

演示代码有时在我们添加新的低级功能时会失败，所以如果这是您第一次运行 demo，为了提高成功的概率，请在继续操作之前先查看 [latest release](https://github.com/solana-labs/solana/releases) ：

```bash
TAG=$(git describe --tags $(git rev-list --tags --max-count=1))
git checkout $TAG
```

### 设置配置

确保在任何节点启动之前都能建立例如投票程序之类的重要程序。 请注意，为了良好的性能，我们在这里使用版本构建的方式。 如果你想要调试构建，只需使用 `cargo build` 并省略 `NDEBUG=1` 命令的一部分。

```bash
cargo build --release
```

运行下面的脚本来初始化网络的创世账本。

```bash
NDEBUG=1 ./multinode-demo/setup.sh
```

### 水龙头

为了验证程序和客户端正常工作，我们需要打开一个水龙头来领取一些测试代币。 水龙头按照 Milton Friedman 的风格“空投”\(免费代币给请求客户\)，然后用于测试交易。

打开水龙头：

```bash
NDEBUG=1 ./multinode-demo/faucet.sh
```

### 单节点测试网

在启动验证节点之前，请确保您获取了想要启动验证节点的机器的 IP 地址，并确保 udp 端口 8000-1000 处于打开状态。

现在在独立的 shell 中启动验证节点：

```bash
NDEBUG=1 ./multinode-demo/bootstrap-validator.sh
```

等待几秒钟进行初始化。 当准备好接收交易时，它会打印“leader ready...”。 如果领导者没有任何测试代币，它将从水龙头请求一些。 在领导者启动之前，水龙头不用一直运行。

### 多节点测试网

如果要运行一个多节点测试网，在启动一个领导者节点后，需要在独立 shell 中加入一些额外的验证节点：

```bash
NDEBUG=1 ./multinode-demo/validator-x.sh
```

如果要在 Linux 上运行增强性能验证节点，必须在系统中安装 [CUDA 10.0](https://developer.nvidia.com/cuda-downloads)：

```bash
./fetch-perf-libs.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/bootstrap-validator.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/validator.sh
```

### 测试网客户端演示

现在您的单节点或多节点测试网已启动并正常运行了，接下来我们发送一些交易！

在另一个 shell 启动客户端：

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh # runs against localhost by default
```

刚刚发生了什么？ 客户端演示将尽最快的速度将 500,000 笔交易发送到测试网。 然后客户端定期连接测试网，看看它当时处理了多少笔交易。 请注意，这个 demo 故意将大量 UDP 数据包发送给网络，因此网络几乎会丢失很大一部分。 这确保了试验网有机会达到 710k TPS。 客户端 demo 在确保测试网不再处理任何其他交易后就停止运行。 您应该看到一些 TPS 数值出现在屏幕上。 在多节点变体中，您也会看到每个验证节点的 TPS 测量值。

### 测试网调试

代码中有一些非常有用的调试消息，您可以在每个模块和每个级别的基础上启用它们。 在运行一个领导者或验证节点之前，请设置正常的 RUST_LOG 环境变量。

例如：

- 要在任意位置启用 `info` 以及只能在 solana::banking_stage 模块中启用 `debug` ：

  ```bash
export RUST_LOG=solana=info,solana::banking_stage=debug
  ```

- 启用 BPF 程序日志记录：

  ```bash
export RUST_LOG=solana_bpf_loader=trace
  ```

一般来说，我们正在使用 `debug` 处理不经常的调试消息， `trace` 处理可能频繁的消息， `info` 用于与性能相关的记录。

您也可以通过 GDB 附加到一个运行过程。 领导者进程命名为 _solana-validator_:

```bash
sudo gdb
attach <PID>
set logging on
thread apply all bt
```

这将把所有线程堆栈跟踪转储到 gdb.txt

## 开发者测试网

在此示例中，我们将把客户端连接到公共测试网。 在测试网上运行验证器，您需要打开 udp 端口 `8000-1000`。

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh --entrypoint devnet.solana.com:8001 --faucet devnet.solana.com:9900 --duration 60 --tx_count 50
```

您可以在 [metrics dashboard](https://metrics.solana.com:3000/d/monitor/cluster-telemetry?var-testnet=devnet) 上观察客户端交易的影响
