---
title: 质押账户结构
---

Solana 上的质押账户可用于将代币委托给网络上的验证节点，从而有可能为质押账户的所有者赚取奖励。 Stake accounts are created and managed differently than a traditional wallet address, known as a _system account_. 系统帐户只能从网络上的其他帐户发送和接收 SOL，而质押帐户需要管理代币委托等更复杂的操作。

Solana 上的质押账户的运作方式也可能与您可能熟悉的其他权益证明区块链网络不同。 本文档描述了 Solana 质押账户的高级结构和功能。

#### 帐户地址

每个质押帐户都有一个唯一的地址，可用于在命令行或任何网络浏览器工具中查找帐户信息。 但是，与其中地址的“密钥对”的持有者控制该钱包的钱包地址不同，与质押账户地址相关联的“密钥对”不一定对该帐户具有任何控制权。 实际上，对于质押账户的地址，甚至可能不存在密钥对或私钥。

质押账户的地址唯一具有密钥对文件的时间是当[使用命令行工具创建质押账户](../cli/delegate-stake.md#create-a-stake-account)时，一个新的密钥对文件被首次创建，仅仅是为了确保质押帐户的地址是新的且唯一的。

#### 理解帐户授权

Certain types of accounts may have one or more _signing authorities_ associated with a given account. 帐户授权用于为其控制的帐户签署某些交易。 这与其他一些区块链网络不同，在其他区块链网络中，与账户地址关联的密钥对的持有者控制着账户的所有活动。

每个质押账户都有两个由其各自地址指定的签名授权，每个授权均被授权对质押账户执行某些操作。

The _stake authority_ is used to sign transactions for the following operations:

- 委托质押
- 停用质押委托
- 分割质押账户，创建一个新的质押账户，其中第一个账户中有一部分资金
- Merging two stake accounts into one
- 设置新的质押授权

The _withdraw authority_ signs transactions for the following:

- 将未委托的质押提取到钱包地址中
- 设置新的提款权限
- 设置新的质押授权

在创建质押账户时，将设置质押授权和提取权限，并且可以随时更改它们以授权新的签名地址。 抵押和提款授权可以是相同的地址，也可以是两个不同的地址。

由于清算质押账户中的代币需要撤回授权密钥对，因此可以更好地控制该帐户，并且如果质押授权密钥对丢失或受到破坏，则可以用来重置质押授权。

在管理质押账户时，确保提取权限不丢失或被盗是至关重要的。

#### 多份委托

每个质押帐户一次只能用于委托一个验证节点。 帐户中的所有代币都是已委托或未委托的，或者正在被委托或未委托的过程中。 要将代币的一部分委托给验证节点，或委托给多个验证节点，您必须创建多个质押账户。

这可以通过从包含一些代币的钱包地址创建多个质押账户来完成，或者通过创建一个大型质押账户，并使用质押授权将帐户拆分为具有您选择的代币余额的多个账户来实现。

可以将相同的质押和提款授权分配给多个质押账户。

#### Merging stake accounts

Two stake accounts that have the same authorities and lockup can be merged into a single resulting stake account. A merge is possible between two stakes in the following states with no additional conditions:

- two deactivated stakes
- an inactive stake into an activating stake during its activation epoch

For the following cases, the voter pubkey and vote credits observed must match:

- two activated stakes
- two activating accounts that share an activation epoch, during the activation epoch

All other combinations of stake states will fail to merge, including all "transient" states, where a stake is activating or deactivating with a non-zero effective stake.

#### Delegation Warmup and Cooldown

When a stake account is delegated, or a delegation is deactivated, the operation does not take effect immediately.

A delegation or deactivation takes several [epochs](../terminology.md#epoch) to complete, with a fraction of the delegation becoming active or inactive at each epoch boundary after the transaction containing the instructions has been submitted to the cluster.

There is also a limit on how much total stake can become delegated or deactivated in a single epoch, to prevent large sudden changes in stake across the network as a whole. Since warmup and cooldown are dependent on the behavior of other network participants, their exact duration is difficult to predict. Details on the warmup and cooldown timing can be found [here](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Lockups

Stake accounts can have a lockup which prevents the tokens they hold from being withdrawn before a particular date or epoch has been reached. While locked up, the stake account can still be delegated, un-delegated, or split, and its stake and withdraw authorities can be changed as normal. Only withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be modified later, by the _lockup authority_ or _custodian_, the address of which is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a balance of 0 SOL is no longer tracked. If a stake account is not delegated and all of the tokens it contains are withdrawn to a wallet address, the account at that address is effectively destroyed, and will need to be manually re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the Solana Explorer by copying and pasting an account address into the search bar.

- http://explorer.solana.com/accounts
