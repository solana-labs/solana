---
title: 质押委托和奖励
---

质押者因帮助验证账本而获得奖励。 他们通过将其质押委托给验证节点来做到这一点。 这些验证节点会进行繁重的工作，广播账本，并将选票发送到每个节点的投票帐户（其中质押者可以委托其质押）。 当出现分叉时，集群的其余部分使用那些质押加权投票来选择一个区块。 验证节点和质押者都需要某种经济激励来发挥作用。 验证节点的硬件需要得到补偿，质押者需要获得奖励来降低其质押风险。 该部分设计详见[质押奖励](../implemented-proposals/staking-rewards.md)。 本章节将描述其实现的基本机制。

## 基本设计

通常的想法是验证节点有一个投票帐户。 投票帐户跟踪验证节点的投票，对验证节点产生的信用进行计数，并提供其他任何针对验证节点的状态。 投票帐户无法悉知委托给它的任何质押，账号本身也没有质押。

单独的质押帐户 \(由质押者创建\) 命名了一个将质押委托的投票帐户。 所产生的奖励与质押的lamports数量成正比。 质押帐户仅由质押人拥有。 此帐户中存储的某些部分Lamport属于质押。

## 被动委托

无需控制投票帐户或向该帐户提交投票的身份进行交互操作，任何数量的质押帐户都可以委托给一个投票帐户。

可以通过将投票帐户pubkey作为 `StakeState::Stake::voter_pubkey` 的所有质押帐户的总和，来计算分配给Vote帐户的总质押。

## 投票和质押账户

奖励过程分为两个链上程序。 投票程序解决了让质押处于罚没的问题状态。 质押计划是奖励池的托管人，提供被动委托。 当显示质押者的代表已参与验证账本时，Stake程序负责向质押者和投票者支付奖励。

### 投票状态

VoteState是验证节点已提交给网络的所有投票的当前状态。 VoteState包含以下状态信息：

- `投票` - 提交的投票数据结构。
- `积分` - 该投票程序在其整个生命期内产生的奖励总额。
- `root_slot` - 达到奖励所需的全部锁定的最后一个插槽。
- `佣金` - VoteState从质押者的Stake帐户获得的任何奖励中抽取的佣金。 这是奖励的百分比上限。
- Account::lamports - 佣金累计获得的lamports。 这些并不算作质押。
- `authorized_voter` - 只有该身份能提交投票。 此字段只能通过身份认证进行修改。
- `node_pubkey` - 在这个帐户中投票的 Solana 节点。
- `authorized_withdrawer` - 负责该账户lamports 实体的身份，独立于帐户地址和授权的投票签名者。

### VoteInstruction::Initialize\(VoteInit\)

- `account[0]` - RW - 选票状态。

  `VoteInit` 带有新投票帐户的 `node_pubkey`, `authorized_porer`, `authorized_withdrawer`, 和 `commission`。

  其他投票状态成员处于默认状态。

### VoteInstruction::Authorize\(Pubkey, VoteAuthorize\)

根据VoteAuthorize参数\(`投票者`或 `提款者`\)，使用新的授权投票人或提款人更新帐户。 交易必须由投票帐户当前的` 授权投票人`或`授权提款人`签名。

- `account[0]` - RW - VoteState。 `VoteState::authorized_voter` 或 `authorized_withdrawer` 设置为 `Pubkey`.

### VoteInstruction::Vote\(Vote\)

- `account[0]` - RW - The VoteState。 `VoteState::lockouts` 和 `VoteState::credit` 是根据投票锁规则更新的，参考 [Tower BFT](../implemented-proposals/tower-bft.md)。
- `account[1]` - RO - `sysvar::slot_hash` 需要验证投票反对的一些 N 最近插槽及其哈希列表。
- `account[2]` - RO - `sysvar::clock` 当前的网络时间，以 slot、epoch 等表示。

### 质押状态（StakeState）

StakeState 通常为这个四种形式之一，StakeState::Uninitialized、StakeState::Initialized、StakeState::Stake 以及 StakeState::RewardsPool。 质押中仅使用前三种形式，但是只有 StakeState::Stake 非常有趣。 所有奖励池都是在创始时创建的。

### StakeState::Stake

StakeState:: Stake 是**质押者**的当前委托首选项，并包含以下状态信息：

- Account::lamports - 可用于质押的 lamports。
- `stake` - 产生奖励的 \(受到预热和冷却的影响\)质押，总是小于或等于 Account::lampport。
- `voter_pubkey` - 把 lamport 委托给 VoteState 实例的 pubkey 。
- `credits_observed` - 在程序的整个生命周期内获得的总积分。
- `activated` - 激活/委托质押的epoch。 所有质押将在预热后计算在内。
- `deactivated` - 停用此质押的epoch，在完全停用帐户之前需要一些冷却epoch，才能提取质押。
- `authorized_staker` - 必须签名委托，激活和停用交易的实体的公钥。
- `authorized_withdrawer` - 负责该帐户实体的身份，独立于帐户的地址和授权质押者。

### StakeState::RewardsPool

为避免单个网络范围内的兑换锁定或争用，在预先确定的密钥下创世的一部分包括256个奖励池，每个密钥具有std::u64::MAX信用额度，以便能够根据积分值满足赎回要求。

质押和奖励池是同一`质押`程序所拥有的帐户。

### StakeInstruction::DelegateStake

质押账户从初始化形式转移到StakeState::Stake形式，或者从已停用(即完全冷却)的StakeState::Stake转变为激活的StakeState::Stake。 这是质押者选择其质押账户Lamport委托给的投票帐户和验证节点节点的方式。 交易必须由质押的`授权质押者`签名。

- `account[0]` - RW - StakeState::Stake 实例。 `StakeState::Stake::credits_observed` 已初始化到 `VoteState::credits`，`StakeState::Stake::voter_pubkey` 已初始化到 `account[1]`。 如果这是首次委托质押，`StakeState::Stake::stake` 会被初始化到账户的 lamports余额，`StakeState::Stake::activated` 被初始化到 Bank epoch，并且 `StakeState::Stake::deactivated` 被初始化到 std::u64::MAX
- `account[1]` - R - VoteState 实例。
- `account[2]` - R - sysvar::clock 账户，包含有关当银行时间的信息。
- `account[3]` - R - sysvar::stakehistory 帐户，包含有关质押历史的信息。
- `account[4]` - R - stake::Config 帐户，负责预热，冷却和罚没配置。

### StakeInstruction::Authorize\(Pubkey, StakeAuthorize\)

根据质押授权参数\(`质押者` 或 `提款人`\)，使用新的授权质押者或提款人更新帐户。 交易必须由质押帐户当前的`授权质押人`或`授权提款者`签名。 任何质押锁定必须已到期，或者锁定托管人也必须签名交易。

- `account[0]` - RW - StakeState。

  `StakeState::authorized_staker` 或 `authorized_withdrawer` 已设置为 `Pubkey`。

### StakeInstruction::Deactivate

质押持有者可能希望从网络中提款。 为此，他必须首先停用自己的质押，然后等待冷却。 交易必须由质押的`授权质押者`签名。

- `account[0]` - RW - 读写正在停用的 StakeState::Stake 实例。
- `account[1]` - R - 带有当前时间的 Bank 的 sysvar::clock 帐户。

StakeState::Stake::deactivated 停用设置为当前时间+冷却时间。 到那个epoch，帐户的质押将减少到零，并且Account::lamports将可以提款。

### StakeInstruction::Withdraw\(u64\)

Lamports会随着时间在一个质押账户中累积，超过已激活质押的任何多余部分都可以提取。 交易必须由质押的`授权提款人`签名。

- `account[0]` - RW - 需要取款的 StakeState::Stake。
- `account[1]` - RW - 应当计入已提取Lamport的帐户。
- `account[2]` - R - 带有当前时间的 Bank sysvar::clock 账户，用于计算质押。
- `account[3]` - R - 来自 Bank 的 sysvar::stake_history 帐户，具有质押预热/冷却历史记录。

## 这种设计的好处

- 所有质押者进行一次投票。
- 清除积分变量对于索取奖励不是必需的。
- 每个委派的质押都可以独立索取奖励。
- 当委托质押要求奖励时，将交纳工作佣金。

## 示例通话流

![被动质押调用流](/img/passive-staking-callflow.png)

## 质押（Staking）奖励

此处概述了验证者奖励制度的具体机制和规则。 通过将质押委托给正确投票的验证人来赚取奖励。 投票不正确会使验证者的质押面临[slashing（罚没）](../proposals/slashing.md)的风险。

### 基础知识

网络获得一部分网络[通胀](../terminology.md#inflation)奖励。 可用于支付时间奖励的Lamports数量是固定的，并且必须根据它们的相对权重和参与度在所有质押节点之间平均分配。 加权单位称为[积分（point）](../terminology.md#point)）。

一个epoch结束以后，才能获得该epoch的奖励。

在每个epoch结束时，将在该epoch期间获得的总积分求和，并用于划分epoch通货膨胀的奖励，求出一个积分值。 该值记录在将时间映射到点值的[sysvar](../terminology.md#sysvar)中。

在赎回期间，质押计划会计算每个epoch的质押所赚取的点数，再乘以该epoch的点值，然后根据奖励账户的佣金设置将该金额的Lamports从奖励账户转移到质押和投票账户中。

### 经济学

一个epoch的积分取决于总的网络参与度。 如果参与epoch缩短，则那些参与epoch的分值会更提高。

### 赚取积分

验证节点对于超出最大锁定范围的每一个正确投票都会获得一票积分，即，每次验证节点的投票帐户从其锁定列表中退出某个版位时，该投票就将成为该节点的根。

委派给该验证程序的抵押人根据其所持质押比例获得积分。 所获得的积分是投票信用和质押的乘积。

### 质押预热、冷却与取回

质押委托以后就不会立即生效。 他们必须首先经过一个预热期。 在此期间，质押的某些部分被视为“有效”，其余部分被视为“激活”。 变化发生在epoch边界上。

质押程序将更改总网络质押的速率限制在质押程序的`config::warmup_rate`中\(在当前实现中设置为每个epoch 25％\)。

每个epoch可以预热的质押数量是前一个epoch的总有效质押，总激活质押，以及质押程序配置的预热率的函数。

冷却时间的工作方式相同。 取消抵押以后，某些部分将被视为“有效”，也被视为“停用”。 随着质押冷却，质押继续获得奖励并有罚没风险，但也可以取回。

引导质押则不需预热。

奖励是针对该epoch的“有效”质押部分进行支付的。

#### 预热示例

考虑在第 N 个 epoch 激活了 1,000 个单一质押的情况，网络预热率为 20％，第 N 个 epoch 的静态总网络质押为 2,000 个。

在第 N + 1 个阶段，可激活的网络数量为400 \(2000的20%\)，在第 N 个阶段，此示例质押是唯一激活的质押，因此有权使用所有的预热质押。

| epoch |  有效的 |   激活中 |   总有效 |  总激活中 |
|:----- | ----:| -----:| -----:| -----:|
| N-1   |      |       | 2,000 |     0 |
| N     |    0 | 1,000 | 2,000 | 1,000 |
| N+1   |  400 |   600 | 2,400 |   600 |
| N+2   |  880 |   120 | 2,880 |   120 |
| N+3   | 1000 |     0 | 3,000 |     0 |

如果在epochN激活了2个质押(X和Y)，他们将按其质押的比例获得20％的一部分。 在每个epoch，每个质押的有效和激活是前一个epoch的状态的函数。

| epoch | 有效 X |  激活 X | 有效 Y | 激活 Y |   总有效 |  总激活中 |
|:----- | ----:| -----:| ----:| ----:| -----:| -----:|
| N-1   |      |       |      |      | 2,000 |     0 |
| N     |    0 | 1,000 |    0 |  200 | 2,000 | 1,200 |
| N+1   |  333 |   667 |   67 |  133 | 2,400 |   800 |
| N+2   |  733 |   267 |  146 |   54 | 2,880 |   321 |
| N+3   | 1000 |     0 |  200 |    0 | 3,200 |     0 |

### 提现

任何时候都只能提取超过有效+激活质押的Lamports。 这意味着在预热期间，实际上无法取回任何抵押。 在冷却期间，超过有效质押的任何代币都可能被取回\(activating == 0\)。 由于赚取的奖励会自动添加到质押中，因此通常只有在停用后才可以提现。

### 锁定

质押账户支持锁定的概念，直到指定的时间，提款账户的余额才能提现。 锁定指定为一个epoch高度，即在可提取质押账户余额之前网络必须达到的最小epoch高度，除非交易也由指定的托管人签署。 此信息在创建质押帐户时收集，并存储在质押帐户状态的Lockup字段中。 更改授权的质押者或提款人也会受到锁定，因为这样的操作实际上就是转移代币。
