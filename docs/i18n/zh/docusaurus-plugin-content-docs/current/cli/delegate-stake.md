---
title: 委托您的质押
---

通过 [ 获取 SOL ](transfer-tokens.md) 以后，您可以通过 _stake_ 将它委托给一个验证节点。 质押（Stake）就是在 _stake account_ 中的代币。 Solana 根据质押权重为验证节点分配投票权重，权重会影响它们在区块链中决定下一个有效交易区块。 然后 Solana 会按周期生成新的 SOL 来奖励质押者和验证节点。 您委托的代币越多，获得的奖励就越高。

## 创建一个质押账户
要委托代币，您首先要将代币转入一个质押帐户。 而要创建一个帐户，您需要一个密钥对： 它的公钥将作为 [质押账户地址](../staking/stake-accounts.md#account-address)。 此处无需密码或加密；此密钥对将在创建密钥账户后被丢弃。

```bash
solana-keygen new --no-passphrase -o stake-account.json
```

输出结果将在文本 `pubkey:` 后面包括该地址。

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

复制公钥并将它安全地存储起来。 在后续创建质押账户的操作中您将随时需要用到它。

创建一个质押账户:

```bash
solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --fee-payer <KEYPAIR>
```

`<AMOUNT>` 的代币从 `<KEYPAIR>` 转到了 stake-account.json 公钥的一个新质押账户。

现在可以丢弃 stake-account.json 文件了。 要授权额外的操作，您可以通过 `--stake-authority` 或 `--rap-authority` 密钥对，而无需使用 stak-account.json。

使用 `solana stake-account` 命令查看新的质押账户：

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

结果大概呈这样：

```text
Total Stake: 5000 SOL
Stake account is undelegated
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

### 设置质押和取款权限
创建账号时，如果需要设置 [质押和提现权限](../staking/stake-accounts.md#understanding-account-authorities)，您可以通过 `--stake-authority` and `--withdraw-authority` 选项或 `solana stake-authorize` 命令来实现。 例如，要设置一个新的质押权限，请运行：

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

这将针对已有的质押账号 `<STAKE_ACCOUNT_ADDRESS>`，通过现有的质押权限 `<KEYPAIR>` 来授权一个新的质押权限 `<PUBKEY>`。

### 高级功能：派生质押账户地址

当委托质押时，你需要将所有密钥账户中的代币委托给某一个验证节点。 而要委托给多个验证节点，您就需要多个质押账户。 为每个帐户创建一个新密钥对并管理那些地址可能比较繁琐。 好在您可以通过 `--seed` 选项来派生多个质押地址：

```bash
solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>
```

`<STRING>` 是一个最多32字节的任意字符串，通常情况下是一个对应该派生账户的数字。 第一个账户是"0"，第二个是 "1"，以此类推。 `<STAKE_ACCOUNT_KEYPAIR>` 公钥发挥基本地址的作用。 该命令将从基础地址和种子字符串中派生一个新地址。 要查看派生出哪个质押地址，请使用 `solana create-address-with-seed`命令：

```bash
solana create-address-with-seed --from <PUBKEY> <SEED_STRING> STAKE
```

`<PUBKEY>` is the public key of the `<STAKE_ACCOUNT_KEYPAIR>` passed to `solana create-stake-account`.

该命令将输出派生地址，可以用于质押操作中的 `<STAKE_ACCOUNT_ADDRESS>` 参数。

## 委托您的质押

想要委托您的质押给某个验证节点，您首先需要它的投票帐号地址。 您可以通过 `solana validators` 命令来查询所有验证节点列表和他们的投票账户：

```bash
solana 验证节点
```

每行的第一列包含验证节点的身份，第二列是投票帐户地址。 选择一个验证节点，并在 `solana delegate-stake` 中使用它的投票帐户地址：

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

质押权限 `<KEYPAIR>` 对地址 `<STAKE_ACCOUNT_ADDRESS>` 进行帐户授权操作。 该质押被委托给投票账户地址 `<VOTE_ACCOUNT_ADDRESS>`。

委托质押后，使用 `solana stake-account` 查看质押账户的变化：

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

您将在输出中看到“Delegated Stake”和“Delegated Vote Account Address”两个新字段。 结果大概呈这样：

```text
Total Stake: 5000 SOL
Credits Observed: 147462
Delegated Stake: 4999.99771712 SOL
Delegated Vote Account Address: CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1
Stake activates starting from epoch: 42
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

## 取消质押

质押委托以后，您可以使用 `solana deactivate-stake` 命令来取消委托的质押：

```bash
solana deactivate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

质押权限 `<KEYPAIR>` 对地址 `<STAKE_ACCOUNT_ADDRESS>` 进行帐户授权操作。

请注意，质押需要几个 epoch 才能“冷却（cool down）”。 在冷却期间进行重新质押的操作将会失败。

## 提现质押

使用 `solana withdraw-stake` 命令将代币转移出质押帐户：

```bash
solana withdraw-stake --withdraw-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

其中，`<STAKE_ACCOUNT_ADDRESS>` 是现有的质押帐户，质押权限 `<KEYPAIR>` 是提现权限， 而 `<AMOUNT>` 是要转账给接收账户 `<RECIPIENT_ADDRESS>` 的代币数量。

## 拆分质押

在现有质押不能取款的时候，您可能想将质押分配给另外的验证节点。 无法取回的原因可能是处于质押、冷却或锁定的状态。 若要将代币从现有质押账户转移到一个新的帐户，请使用 `solana split-stake` 命令：

```bash
solana split-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

其中，`<STAKE_ACCOUNT_ADDRESS>` 是现有的质押帐户，质押权限 `<KEYPAIR>` 是质押账户的权限， `<NEW_STAKE_ACCOUNT_KEYPAIR>` 是新账户的密钥对，`<AMOUNT>` 是要转账给新账户的代币数量。

若要将质押账户拆分到派生账户地址，请使用 `--seed` 选项。 详情请参阅 [衍生质押账户地址](#advanced-derive-stake-account-addresses)。
