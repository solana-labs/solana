---
title: 术语
---

在整个文档中使用以下术语。

## 帐户（account）

由[公钥](terminology.md#public-key)寻址并带有[lamports](terminology.md#lamport)跟踪其生存期的持久文件。

## 应用程序（app）

与Solana集群交互的前端应用程序。

## 账户状态（bank state）

以给定的[刻度高度](terminology.md#tick-height)解释账本上所有程序的结果。 它至少包含所有持有非零[原生代币](terminology.md#native-tokens) 的所有[帐户](terminology.md#account)的集合。

## 区块（block）

在[投票](terminology.md#ledger-vote)所覆盖的账本上的一组连续[条目](terminology.md#entry)。 一个[领导者](terminology.md#leader)最多产生一个区块[插槽](terminology.md#slot)。

## 区块哈希（blockhash）

在一定[区块高度](terminology.md#block-height)，[账本](terminology.md#ledger)产生的一组连续的[哈希](terminology.md#hash)。 取自插槽最后的 [条目id](terminology.md#entry-id)

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

代表2/3[质押](terminology.md#stake)的节点达成一个公共[根](terminology.md#root)。

## 分叉（fork）

[账本](terminology.md#ledger)从通用条目派生出来，但后来又不相同。

## 创世区块（genesis block）

区块链中的第一个[区块](terminology.md#block)。

## 创世配置（genesis config）

用于为[创世区块](terminology.md#genesis-block)准备[账本](terminology.md#ledger)的配置文件。

## 哈希（hash）

字节序列的数字指纹。

## 通货膨胀（inflation）

随着时间的流逝，代币供应的增加用于资助验证奖励和为Solana的持续发展提供资金。

## 指令（instruction）

[客户端](terminology.md#client)可以在一笔[交易](terminology.md#transaction)中包括的[程序](terminology.md#program)最小单元。

## 密钥对（keypair）

[公钥](terminology.md#public-key)和相应的[私钥](terminology.md#private-key)。

## lamport

微量的[原生代币](terminology.md#native-token)，其值为 0.000000001 [sol](terminology.md#sol)。

## 领导者（leader）

[验证程序](terminology.md#validator)将[条目](terminology.md#entry)追加到[账本](terminology.md#ledger)时的角色。

## 领导者时间表（leader schedule）

[验证节点](terminology.md#validator)的[公钥](terminology.md#public-key)序列。 集群使用领导者时间表来随时确定哪个验证者作为[领导者](terminology.md#leader)。

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

奖励制度中的加权[积分](terminology.md#credit)。 在[验证节点](terminology.md#validator) [奖励制度](cluster/stake-delegation-and-rewards.md)中，赎回过程中所获得的[质押](terminology.md#stake)点数是所获得的[投票积分](terminology.md#vote-credit)与所抵押lamports数量的乘积。

## 私钥（private key）

[密钥对](terminology.md#keypair)的私钥。

## 程序（program）

解释[指令](terminology.md#instruction)的代码。

## 程序ID（program id）

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

R(32字节) 和S(32字节) 的64字节ed25519签名。 要求R为不小于小数的压缩Edwards点，而S为 0 <= S < L范围内的标量。此要求确保不具有签名延展性。 每笔交易必须至少有一个用于[费用账户](terminology#fee-account)的签名。 因此，交易中的第一个签名可以被视为[交易ID](terminology.md#transaction-id)。

## 插槽（slot）

[领导者](terminology.md#leader)提交交易并产生[区块](terminology.md#block)的时间段。

## 智能合约（smart contract）

一组约束一旦满足，就会向程序发出信号，通知它们允许某些预定义的帐户更新。

## sol 代币

由Solana公司认可的[集群](terminology.md#cluster)跟踪的[原生代币](terminology.md#native-token)。

## 质押（stake）

如果可以证明恶意[验证节点](terminology.md#validator)的行为，代币将被没收给[集群](terminology.md#cluster)。

## 绝大多数（supermajority）

[群集](terminology.md#cluster)的 2/3。

## 系统变量（sysvar）

Runtime 提供的合成[帐户](terminology.md#account)，允许程序访问网络状态，例如当前滴答高度，奖励[积分](terminology.md#point)值等。

## 轻客户端（thin client）

一种信任它正在与有效的[集群](terminology.md#cluster)通信的[客户端](terminology.md#client)类型。

## 滴答（tick）

估算壁钟持续时间的账本[条目](terminology.md#entry)。

## 滴答高度（tick height）

[账本](terminology.md#ledger) 第 N 次 [滴答](terminology.md#tick)。

## 代号（token）

一组稀有、可替代的代币。

## 每秒交易次数（tps）

每秒 [交易](terminology.md#transaction) 的次数。

## 交易（transaction）

由 [客户端](terminology.md#client) 使用一个或多个 [密钥对](terminology.md#keypair)签名的一个或多个 [指令](terminology.md#instruction)，并在只有两个可能的结果的情况下自动执行：成功或失败。

## 交易 id

[交易](terminology.md#transaction)中的第一个[签名](terminology.md#signature)，可用于在整个[账本](terminology.md#ledger)中唯一地标识交易。

## 交易确认（transaction confirmations）

自从交易被接受到[账本](terminology.md#ledger)以来[已确认的区块数](terminology.md#confirmed-block)。 交易在该区块成为[根](terminology.md#root)时完成。

## 交易条目（transactions entry）

一组可以并行执行的 [交易](terminology.md#transaction)。

## 验证节点（validator）

[群集](terminology.md#cluster)的全程参与者，负责验证[账本](terminology.md#ledger)并产生新的[区块](terminology.md#block)。

## 可验证延迟方程（VDF）

请参考 [可验证延迟方程](terminology.md#verifiable-delay-function)。

## 可验证延迟方程（verifiable delay function）

一个需要花费固定时间执行的函数，它会产生一个运行证明，然后可以在比生产所花费的时间更少的时间内对其进行验证。

## 投票（vote）

请参考 [账本投票](terminology.md#ledger-vote)。

## 投票积分（vote credit）

[验证程序](terminology.md#validator) 获得的奖励。 当验证节点达到 [根](terminology.md#root) 时，将投票积分授予其投票帐户中的验证节点。

## 钱包（wallet）

[密钥对](terminology.md#keypair) 的集合。

## 预热期（warmup period）

[质押](terminology.md#stake)已经委托并开始逐渐生效过程中的一些[epoch](terminology.md#epoch)。 在此期间，质押被认为是“激活”的。 相关的更多信息请参考：[预热和冷却](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)。
