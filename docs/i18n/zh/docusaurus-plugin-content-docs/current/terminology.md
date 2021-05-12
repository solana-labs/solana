---
title: 术语
---

在整个文档中使用以下术语。

## 帐户（account）

由[公钥](terminology.md#public-key)寻址并带有[lamports](terminology.md#lamport)跟踪其生存期的持久文件。

## 应用程序（app）

与 Solana 集群交互的前端应用程序。

## 账户状态（bank state）

以给定的[刻度高度](terminology.md#tick-height)解释账本上所有程序的结果。 它至少包含所有持有非零[原生代币](terminology.md#native-tokens) 的所有[帐户](terminology.md#account)的集合。

## 区块（block）

在[投票](terminology.md#ledger-vote)所覆盖的账本上的一组连续[条目](terminology.md#entry)。 一个[领导者](terminology.md#leader)最多产生一个区块[插槽](terminology.md#slot)。

## 区块哈希（blockhash）

在一定[区块高度](terminology.md#block-height)，[账本](terminology.md#ledger)产生的一组连续的[哈希](terminology.md#hash)。 取自插槽最后的 [条目 id](terminology.md#entry-id)

## 区块高度（block height）

当前区块下方的 [区块](terminology.md#block) 个数。 [创世区块](terminology.md#genesis-block) 之后的第一个区块的高度为一。

## 启动验证节点（bootstrap validator）

第一个生成 [区块](terminology.md#block) 的 [验证节点](terminology.md#validator)。

## CBC 区块（CBC block）

账本中最小的加密区块，一个加密账本分段由许多 CBC 区块组成。 `ledger_segment_size / cbc_block_size` 是精确的。

## 客户端（client）

使用 [集群](terminology.md#cluster) 的一个 [节点](terminology.md#node)。

## 集群（cluster）

一组 [验证节点](terminology.md#validator) 维持的一个 [账本](terminology.md#ledger)。

## 确认时间（confirmation time）

[领导者](terminology.md#leader) 创建一个 [条目](terminology.md#tick) 并创建一个 [确认的区块](terminology.md#confirmed-block) 之间的壁时钟持续时间。

## 确认的区块（confirmed block）

一个 [区块](terminology.md#block) 已经获得了[绝大多数](terminology.md#supermajority) 的 [账本投票 ](terminology.md#ledger-vote)，其中账本解释器同领导者相匹配。

## 控制面板（control plane）

所有[节点](terminology.md#node)的八卦网络都连接在一个[集群](terminology.md#cluster)内。

## 冷却期（cooldown period）

[质押](terminology.md#stake)后，将逐渐取消提现后的一些[纪元](terminology.md#epoch)。 在此期间，质押被认为是“停用的”。 相关的更多信息请参考：[预热和冷却](implemented-proposals/staking-rewards.md#stake-warmup-cooldown-withdrawal)。

## 积分（credit）

请参阅[投票积分](terminology.md#vote-credit)。

## 数据面板（data plane）

用于有效验证[条目](terminology.md#entry)并达成共识的多播网络。

## 无人机（drone）

一项链下服务，充当用户私钥的保管人。 它通常用于验证和签名交易。

## 条目（entry）

[账本](terminology.md#ledger)上的条目是一个[滴答](terminology.md#tick)或[交易条目](terminology.md#transactions-entry)。

## 条目 id

在条目的最后内容上显示一个预图像抗性的 [哈希](terminology.md#hash) ，该条目起着 [条目的](terminology.md#entry) 全局唯一标识符的作用。 哈希作为下列证据：

- 指定的交易是包含在条目中的交易
- 条目相对于账本中其他[交易](terminology.md#transaction)的位置
- 条目与 [账本](terminology.md#ledger)中其他条目的位置

请参阅 [历史证明](terminology.md#proof-of-history)。

## 纪元（epoch）

[领导者时间表](terminology.md#leader-schedule)有效的时间（即[插槽](terminology.md#slot)数）。

## 费用账户（fee account）

交易中的费用帐户是支付将交易包括在账本中所需成本的帐户。 这是交易中的第一个帐户。 该帐户必须在交易中声明为可读写(可写)，因为为交易付款会减少帐户余额。

## 最终性（finality）

代表 2/3[质押](terminology.md#stake)的节点达成一个公共[根](terminology.md#root)。

## 分叉（fork）

[账本](terminology.md#ledger)从通用条目派生出来，但后来又不相同。

## 创世区块（genesis block）

区块链中的第一个[区块](terminology.md#block)。

## 创世配置（genesis config）

用于为[创世区块](terminology.md#genesis-block)准备[账本](terminology.md#ledger)的配置文件。

## 哈希（hash）

字节序列的数字指纹。

## 通货膨胀（inflation）

随着时间的流逝，代币供应的增加用于资助验证奖励和为 Solana 的持续发展提供资金。

## 指令（instruction）

[客户端](terminology.md#client)可以在一笔[交易](terminology.md#transaction)中包括的[程序](terminology.md#program)最小单元。

## 密钥对（keypair）

[公钥](terminology.md#public-key)和相应的[私钥](terminology.md#private-key)。

## lamport

微量的[原生代币](terminology.md#native-token)，其值为 0.000000001 [sol](terminology.md#sol)。

## 领导者（leader）

[验证程序](terminology.md#validator)将[条目](terminology.md#entry)追加到[账本](terminology.md#ledger)时的角色。

## 领导者时间表（leader schedule）

A sequence of [validator](terminology.md#validator) [public keys](terminology.md#public-key) mapped to [slots](terminology.md#slot). 集群使用领导者时间表来随时确定哪个验证者作为[领导者](terminology.md#leader)。

## 账本（ledger）

包含[客户端](terminology.md#client)签名的[交易](terminology.md#transaction)的[条目](terminology.md#entry)列表。 从概念上讲，这可以追溯到[创世区块](terminology.md#genesis-block)，但是实际[验证节点](terminology.md#validator)的账本可能只有较新的[区块](terminology.md#block)才能保存存储使用情况，因为较旧的区块不需要通过设计来验证将来的区块。

## 账本投票（ledger vote）

在一定[滴答高度](terminology.md#tick-height)，[银行状态](terminology.md#bank-state)产生的一组[哈希](terminology.md#hash)。 它包括[验证者](terminology.md#validator)确认其已收到的[区块](terminology.md#block)已被验证的承诺，以及承诺在特定时间段（[锁定期](terminology.md#lockout)）内不投票给有冲突的[区块](terminology.md#block)(即[分叉](terminology.md#fork)) 的承诺。

## 轻量级客户端（light client）

一种可以验证其指向有效[集群](terminology.md#cluster)的[客户端](terminology.md#client)。 它比[瘦客户端](terminology.md#thin-client)执行更多的账本验证工作，但比[验证节点](terminology.md#validator)的工作更少。

## 加载程序（loader）

能够解释其他链上程序的二进制编码的[程序](terminology.md#program)。

## 锁定时间（lockout）

[验证节点](terminology.md#validator)无法对另一个[分叉](terminology.md#fork)进行[投票](terminology.md#ledger-vote)的持续时间。

## 原生代币（native token）

用于跟踪集群中[节点](terminology.md#node)完成的工作的[代币](terminology.md#token)。

## 节点（node）

参与到一个[群集](terminology.md#cluster)的一台机器。

## 节点数量（node count）

参与到一个[集群](terminology.md#cluster)的[验证节点](terminology.md#validator)数量。

## 历史证明（PoH）

请参阅 [历史证明](terminology.md#proof-of-history)。

## 点数（point）

奖励制度中的加权[积分](terminology.md#credit)。 在[验证节点](terminology.md#validator) [奖励制度](cluster/stake-delegation-and-rewards.md)中，赎回过程中所获得的[质押](terminology.md#stake)点数是所获得的[投票积分](terminology.md#vote-credit)与所抵押 lamports 数量的乘积。

## 私钥（private key）

[密钥对](terminology.md#keypair)的私钥。

## 程序（program）

解释[指令](terminology.md#instruction)的代码。

## 程序 ID（program id）

[帐户](terminology.md#account) 公钥包含的一个 [程序](terminology.md#program)。

## 历史证明（Proof of History）

一堆证明， 其中每一种情况都证明了在出示证据之前存在某些数据，而且在前一证据之前确切的时间已经过一段时间。 例如一个 [VDF](terminology.md#verifiable-delay-function)，它可以在比生成时间更短的时间内验证历史证明。

## 公钥（public key）

[密钥对](terminology.md#keypair)的公钥。

## 根（root）

在[验证程序](terminology.md#validator)上已达到最大[锁定](terminology.md#lockout)的[区块](terminology.md#block)或[插槽](terminology.md#slot)。 根是最高的块，它是验证节点上所有活跃分叉的祖先。 根的所有祖先区块也是暂时的根。 不是祖先，也不属于根后代的区块将从共识中排除并丢弃。

## 运行时（runtime）

[验证节点](terminology.md#validator)的负责[程序](terminology.md#program)执行的组件。

## 碎片（shred）

一小部分[区块](terminology.md#block)；[验证节点](terminology.md#validator)之间发送的最小单位。

## 签名（signature）

R(32 字节) 和 S(32 字节) 的 64 字节 ed25519 签名。 要求 R 为不小于小数的压缩 Edwards 点，而 S 为 0 <= S < L 范围内的标量。此要求确保不具有签名延展性。 每笔交易必须至少有一个用于[费用账户](terminology#fee-account)的签名。 因此，交易中的第一个签名可以被视为[交易 ID](terminology.md#transaction-id)。

## skipped slot

A past [slot](terminology.md#slot) that did not produce a [block](terminology.md#block), because the leader was offline or the [fork](terminology.md#fork) containing the slot was abandoned for a better alternative by cluster consensus. A skipped slot will not appear as an ancestor for blocks at subsequent slots, nor increment the [block height](terminology#block-height), nor expire the oldest `recent_blockhash`.

Whether a slot has been skipped can only be determined when it becomes older than the latest [rooted](terminology.md#root) (thus not-skipped) slot.

## slot

The period of time for which each [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).

Collectively, slots create a logical clock. Slots are ordered sequentially and non-overlapping, comprising roughly equal real-world time as per [PoH](terminology.md#proof-of-history).

## smart contract

A set of constraints that once satisfied, signal to a program that some predefined account updates are permitted.

## sol

The [native token](terminology.md#native-token) tracked by a [cluster](terminology.md#cluster) recognized by the company Solana.

## stake

Tokens forfeit to the [cluster](terminology.md#cluster) if malicious [validator](terminology.md#validator) behavior can be proven.

## supermajority

2/3 of a [cluster](terminology.md#cluster).

## sysvar

A synthetic [account](terminology.md#account) provided by the runtime to allow programs to access network state such as current tick height, rewards [points](terminology.md#point) values, etc.

## thin client

A type of [client](terminology.md#client) that trusts it is communicating with a valid [cluster](terminology.md#cluster).

## tick

A ledger [entry](terminology.md#entry) that estimates wallclock duration.

## tick height

The Nth [tick](terminology.md#tick) in the [ledger](terminology.md#ledger).

## token

A scarce, fungible member of a set of tokens.

## tps

[Transactions](terminology.md#transaction) per second.

## transaction

One or more [instructions](terminology.md#instruction) signed by the [client](terminology.md#client) using one or more [keypairs](terminology.md#keypair) and executed atomically with only two possible outcomes: success or failure.

## transaction id

The first [signature](terminology.md#signature) in a [transaction](terminology.md#transaction), which can be used to uniquely identify the transaction across the complete [ledger](terminology.md#ledger).

## transaction confirmations

The number of [confirmed blocks](terminology.md#confirmed-block) since the transaction was accepted onto the [ledger](terminology.md#ledger). A transaction is finalized when its block becomes a [root](terminology.md#root).

## transactions entry

A set of [transactions](terminology.md#transaction) that may be executed in parallel.

## validator

A full participant in the [cluster](terminology.md#cluster) responsible for validating the [ledger](terminology.md#ledger) and producing new [blocks](terminology.md#block).

## VDF

See [verifiable delay function](terminology.md#verifiable-delay-function).

## verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof that it ran, which can then be verified in less time than it took to produce.

## vote

See [ledger vote](terminology.md#ledger-vote).

## vote credit

A reward tally for [validators](terminology.md#validator). A vote credit is awarded to a validator in its vote account when the validator reaches a [root](terminology.md#root).

## wallet

A collection of [keypairs](terminology.md#keypair).

## warmup period

Some number of [epochs](terminology.md#epoch) after [stake](terminology.md#stake) has been delegated while it progressively becomes effective. During this period, the stake is considered to be "activating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
