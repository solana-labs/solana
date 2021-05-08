---
title: 投票帐户管理
---

此页面描述了如何设置一个链上的 _投票账户_。  如果您计划在Solana上运行验证器节点，则需要创建一个投票帐户。

## 创建一个投票帐户
可以使用[create-vote-account](../cli/usage.md#solana-create-vote-account)命令创建一个投票帐户。 在首次创建投票器时或在运行验证器之后可以配置投票帐户。  用户可以更改投票帐户的所有方面，除了[投票帐户地址](#vote-account-address)，这个地址在整个生命周期内都是固定的。

### 配置现有的投票账户
 - 要更改 [验证节点身份](#validator-identity)，请使用[vote-update-validator](../cli/usage.md#solana-vote-update-validator)。
 - 要更改 [投票授权](#vote-authority)，请使用[vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter)。
 - 要更改 [提款权限](#withdraw-authority)，请使用[vote-authorize-withdrawer](../cli/usage.md#solana-vote-authorize-withdrawer)。
 - 要更改 [佣金](#commission)，请使用[vote-update-commissio](../cli/usage.md#solana-vote-update-commission)。

## 投票帐户结构

### 调查帐户地址
作为密钥可以对文件的公钥的地址，或对文件的公钥和种子字符串的派生地址创建投票帐户。

投票帐户的地址不需要签名任何交易，而仅用于查找帐户信息。

当某人想要[委托质押账户中的代币](../staking.md)时，将委托命令指向代币持有者要委托给其的验证节点的投票账户地址。

### 验证节点身份

_验证节点身份_是一个系统帐户，用于支付提交给该投票帐户的所有投票交易费用。 由于希望验证节点对收到的大多数有效区块进行投票，因此验证节点身份帐户经常(可能每秒多次) 签名交易并支付费用。  验证节点身份识别密钥必须被作为“热钱包（Hot Wallet）”存储在验证器正在同一系统运行的“密钥对”文件中。

因为热钱包通常不如线下或“冷”钱包安全，所以验证程序操作者可以选择在身份帐户上仅存储足够的SOL，以支付一段时间（例如几周或几个月）的投票费用。  验证节点身份帐户可以定期从更安全的钱包中充值。

如果验证节点节点的磁盘或文件系统受到破坏或损坏，这种做法可以降低资金损失的风险。

创建投票帐户时，需要提供验证节点身份。 在使用[vote-update-validator](../cli/usage.md#solana-vote-update-validator)命令创建帐户后，也可以更改验证节点身份。

### 投票授权

_Vote Authority（投票授权）_密钥对用于签名验证节点节点要提交给集群的每一个投票交易。  正如在本文档后面的内容，这不一定必须与验证节点身份唯一。  因为投票授权（如验证节点身份）经常签名交易，所以它也必须是与验证节点进程相同的文件系统上的热密钥对。

可以将投票权设置为与验证节点身份相同的地址。 如果验证节点身份也是投票授权，则每次投票交易仅需要一个签名，以便签名投票并支付交易费用。  因为Solana上的交易费用是按签名进行评估的，所以与将投票权和验证节点身份设置到两个不同的帐户相比，只有一个签名者而不是两个签名者将导致仅需支付一半的交易费用。

创建投票帐户时可以设置投票权限。  如果未提供，则默认为：为其分配与验证节点身份相同的名称。 稍后可以使用[vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter)命令更改投票权限。

每个epoch最多可以更改一次投票权限。  如果使用[vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter)更改了权限，则该权限直到下一个epoch开始时才生效。 为了支持投票签名的平稳过渡，`solana-validator`允许多次指定`authorized-voter`参数。  这样，当网络到达验证节点的投票权限，帐户更改的边界时，验证节点进程就可以继续成功进行投票。

### 提现授权

_Withdraw Authority（提款授权）_密钥对用于使用[withdraw-from-vote-account](../cli/usage.md#solana-withdraw-from-vote-account)命令从投票帐户中提取资金。  验证节点获得的任何网络奖励都将被存入投票帐户，并且只能通过使用提款授权授权的密钥对进行签名才能进行检索。

提款授权还要求提款授权授权在任何交易中签字，以更改投票帐户的[佣金](#commission)，并更改投票帐户上的验证节点身份。

由于投票帐户可能会产生大量余额，因此请考虑将提款授权“密钥对”保留在线下/冷钱包中，因为这时候不需要签名频繁交易。

可以在投票帐户创建时使用`--authorized-withdrawer`选项设置提款权限。  如果未提供，默认情况下，验证节点身份将设置为提取权限。

稍后可以使用[vote-authorize-withdrawer](../cli/usage.md#solana-vote-authorize-withdrawer)命令更改提现权限。

### 佣金

_Commission（佣金）_是验证节点获得网络奖励的百分比，该百分比已存入验证节点的投票帐户。  其余的奖励将分配给该投票帐户所有的质押帐户，并与每个质押帐户的激活质押权重成比例。

例如，如果投票帐户设置10%的佣金，则对于给定时期内获得的所有奖励，10%的奖励将在下个epoch的第一段中存入该验证节点的投票帐户。 剩余的90％将马上作为激活的质押存入委托质押账户。

验证节点可以选择设置较低的佣金来吸引更多的质押委托，因为较低的佣金会降低委托人的成本。  但是由于设置和运行验证程序节点存在一定的基础成本，因此理想情况下验证程序必须设置一定量的佣金，用来支付这些费用。

可以在创建投票帐户时使用 `--commission` 选项设置佣金。 如果未提供，它将默认为100％，这将导致所有奖励存到投票帐户，而任何奖励都不会传到任何委托的质押帐户。

以后也可以使用[vote-update-commission](../cli/usage.md#solana-vote-update-commission)命令更改佣金。

设置佣金时，仅接受设置[0-100] 中的整数值。 整数代表佣金的百分点，因此，创建带有`--commission 10`的帐户将设置10％的佣金。

## 更换密钥
处理实时验证节点时，更改投票帐户授权密钥需要特殊处理。

### 投票帐号验证节点身份

您需要访问投票帐户的_withdraw authority（提款权限）_密钥对，才能更改验证节点身份。  以下步骤假定`~/withdraw-authority.json`是该密钥对。

1. 创建新的验证节点身份密钥对，即`solana-keygen new -o ~/new-validator-keypair.json`。
2. 确保已为新的身份帐户`solana transfer ~/new-validator-keypair.json 500`提供资金。
3. 运行`solana vote-update-validator ~/vote-account-keypair.json ~/new-validator-keypair.json ~/withdraw-authority.json`来修改投票账户中的验证节点身份
4. 使用用于`--identity`参数的新身份密钥对重新启动验证节点。

### 投票帐户授权的投票者
更改_vote authority_密钥对只能在epoch边界进行，并且需要对`solana-validator`进行一些附加参数以实现无缝迁移。

1. 运行`solana epoch-info`。  如果当前epoch中没有剩余时间，请考虑等待下一个时，以使您的验证节点有足够的时间重新启动并跟上。
2. 创建新的投票授权密钥对，即`solana-keygen new -o ~/new-vote-authority.json`。
3. 对于当前的_vote authority_密钥对，可通过运行`solana vote-account ~/vote-account-keypair.json`来确定。  它可能是验证节点的身份帐户(默认) 或其他一些密钥对。  以下步骤假定 `~/validator-keypair.json` 是该密钥对。
4. 运行`solana vote-authorize-voter ~/vote-account-keypair.json ~/validator-keypair.json ~/new-vote-authority.json`。 新的投票授权计划在下一个epoch开始生效。
5. 现在需要用旧的和新的投票授权密钥对重新启动`solana-validator`，以便它可以在下一个epoch平稳过渡。 在重新启动时添加两个参数：`--authorized-voter ~/validator-keypair.json，
--authorized-voter ~/new-vote-authority.json`
6. 集群到达下一个epoch后，请删除`--authorized-voter ~/validator-keypair.json`参数，并重新启动`Solana-validator`，因为不再需要旧的投票授权密钥对。


### 投票帐户授权提款人
无需特殊处理。  根据需要使用 `solana vote-authorize-withdrawer` 命令。
