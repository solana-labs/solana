---
title: 质押
---

** 在默认情况下，您的验证节点是没有质押的。** 这意味着无法当选领导者。

## 监测更新

如果要委托质押，首先请确保您的验证程序正在运行，并且跟上集群。 验证节点启动后，可能需要一些时间来跟上。 请使用 `catchup` 命令来监控该过程：

```bash
solana catchup ~/validator-keypair.json
```

在验证节点跟上之前，它无法成功投票，因此也无法委托质押。

另外，如果集群插槽速度比您的快，那么你很可能永远都跟不上最新状态。 这通常意味着您的验证节点与群集其余部分之间出现某种网络问题。

## 创建质押密钥

如果还没有质押密钥，那么您需要新创建一个。 如果你完成了这个步骤，那么在 Solana 运行目录中将出现 “validator-stake-keypair.json”。

```bash
solana-keygen new -o ~/validator-stake-keypair.json
```

## 委托您的质押

在首先创建质押帐户之前，现在先把 1 SOL 委托给您的验证节点：

```bash
solana create-stake-account ~/validator-stake-keypair.json 1
```

然后将这个质押委托给您的验证节点：

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/vote-account-keypair.json
```

> 不要委托您剩余的 SOL，因为验证程序需要用这些代币进行投票。

任何时候都可以使用相同的命令将质押重新委托到另一个节点，但每个 epoch 只能够换一次：

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/some-other-vote-account-keypair.json
```

假设节点正在投票，现在您已经开始运行并产生验证节点奖励。 在每个 epoch 结束阶段，奖励会自动进行发放。

根据投票帐户中设置的佣金率，所赚取的lamports奖励将在您的质押帐户和投票帐户之间分配。 奖励只能在验证节点启动并运行时获得。 此外，验证节点一旦投入，便成为网络的重要组成部分。 为了安全地从网络中删除验证节点，请先停用其质押。

在每个时段的末尾，验证程序都将发送投票交易。 这些投票交易由验证者的身份帐户中的lamports支付。

这是正常交易，因此将收取标准交易费。 交易费用范围由创世区块定义。 实际费用将根据交易量而变动。 您可以在提交交易之前通过[RPC API “getRecentBlockhash”](developing/clients/jsonrpc-api.md#getrecentblockhash)确定当前费用。

点击这里了解更多关于 [交易费的信息](../implemented-proposals/transaction-fees.md)。

## 验证节点预热

为了抵制对共识的各种攻击，新的质押委托需要经过一个[预热](/staking/stake-accounts#delegation-warmup-and-cooldown)阶段。

在预热期间，您可以通过以下命令来监控一个验证节点的委托：

- 查看您的投票账户：`solana vote-account - keypair.json`，这将显示验证节点提交给网络的所有投票的当前状态。
- 查看您的质押账户、委托偏好以及您的质押细节：`solana stock-account ~/validator-stake-keypair.json`
- `Solana validators` 显示当前所有验证程序的活跃质押，包括您的
- `solana stake-history` 展示了最近epochs中预热和冷却的历史
- 在您的验证节点查找日志消息，指明您的下一位领导者插槽：`[2019-09-27T20：16：00.319721164Z INFO solana_core:::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> 投票并在出块高度重置PoH####。 我的下一个领导者插槽是 ####`
- 质押预热完毕后，您可以通过运行 `solana validators` 来看到验证程序中列出的一个质押余额。

## 监视您质押的验证节点

确认你的验证节点变成了一个 [领导者](../terminology.md#leader)

- 在您的验证节点被追上后，使用`solanabalance`命令来监控收入，因为您的验证人被选为领导者并收取交易费用
- Solana节点提供了许多有用的JSON-RPC方法，来返回有关网络和验证节点参与的信息。 通过使用curl\(或您选择的另一个http客户端) 发出请求，并在JSON-RPC格式的数据中指定所需的方法。 例如：

```bash
  // 请求
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // 结果
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

有用的 JSON RPC 方法：

- `getEpochInfo`[一个纪元](../terminology.md#epoch) 是一段时间，即 [slots](../terminology.md#slot)的数量，一个 [领导者计划](../terminology.md#leader-schedule) 在这个期间有效。 这将告诉你当前的epoch以及集群经过了多长时间。
- `getVoteAccounts` 这将告诉您，目前验证程序有多少活跃的质押。 验证节点的质押百分比是在epoch刚开始时激活的。 您可以 [在这里](../cluster/stake-delegation-and-rewards.md) 了解更多关于Solana质押的信息。
- `getLeaderSchedule` 在任何时候，网络只需要一个验证节点来生成账本条目。 目前被选定生成账本条目的 [验证节点](../cluster/leader-rotation.md#leader-rotation) 被称为“领导者”。 这将返回当前激活质押的完整领导者时间表\(按插槽来算\) ，身份公钥将在此处显示一次或多次。

## 停用质押

在从集群中分离验证节点之前，应通过运行以下命令停用先前委托的质押：

```bash
solana deactivate-stake ~/validator-stake-keypair.json
```

质押不会立即停用，而是类似于预热的方式先进行冷却。 您的验证程序应在冷却时保持在群集上。 在冷却的同时，您的质押将继续获得奖励。 只有在质押冷却之后，才能安全关闭验证节点或从网络中撤出。 冷却时间可能需要几个epoch，具体取决于活跃质押及其规模。

请注意，质押帐户只能使用一次，所以在停用后， 使用CLI客户端的 `withdraw-stake` 命令来恢复先前有质押的份额。
