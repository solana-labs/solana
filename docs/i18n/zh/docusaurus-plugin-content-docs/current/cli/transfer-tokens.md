---
title: 发送和接收代币
---

该网页展示了如何通过命令行钱包，使用命令行工具接收和发送 SOL代币，例如 [纸钱包](../wallet-guide/paper-wallet.md)， [文件系统钱包](../wallet-guide/file-system-wallet.md), 或[硬件钱包](../wallet-guide/hardware-wallets.md). 在开始之前，请确认您已经创建了一个钱包，并且可以访问其地址 (pubkey) 和签名密钥对。 请查看我们的[约定来输入不同钱包类型的密钥对](../cli/conventions.md#keypair-conventions).

## 测试您的钱包

在与其他人分享公钥前，您可能需要首先确认密钥的有效性，并确保真正拥有相应的私钥。

在这个例子中，我们将在第一个钱包的基础上再创建另一个钱包，然后转入一些代币。 这个步骤确保您可以在该钱包正常发送和接收代币。

该测试将通过我们的开发者测试网（称为devnet）。 测试网发行的代币**并没有**实际价值，所以无需担心资产损失。

#### 获取一些空投代币，开始操作

首先，在测试网给您的钱包_空投_ 一些虚拟代币。

```bash
solana airdrop 10 <RECIPIENT_ACCOUNT_ADDRESS> --url https://api.devnet.solana.com
```

其中，用您的 base58-encoded 公钥/钱包地址替换此处的 `<RECIPIENT_ACCOUNT_ADDRESS>`文本。

#### 检查钱包余额

通过检查帐户余额确认空投已经成功。 输出值应当为 `10 SOL`:

```bash
solana balance <ACCOUNT_ADDRESS> --url https://api.devnet.solana.com
```

#### 创建第二个钱包地址

我们需要一个新地址来接收代币。 创建第二个密钥对并记录其公钥：

```bash
solana-keygen new --no-passphrase --no-outfile
```

输出将在文本 `pubkey:` 后面包括该地址。 复制该地址。 我们在下一步中要用到它。

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

您还可以通过下述方式创建任何类型的一个(或多个)钱包： [paper](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses), [file system](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses), 或者 [hardware](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet).

#### 将代币从您的第一个钱包转到第二个地址

接下来，通过发送来证明你拥有空投代币。 Solana 集群只有在您用交易发送方公钥对应的私钥签名时，才会接受交易。

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 5 --url https://api.devnet.solana.com --fee-payer <KEYPAIR>
```

其中，用第一个钱包的秘钥对的路径替换 `<KEYPAIR>`，用第二个钱包地址替换 `<RECIPIENT_ACCOUNT_ADDRESS>`。

使用 `solana balance` 确认余额已经更新：

```bash
solana balance <ACCOUNT_ADDRESS> --url http://api.devnet.solana.com
```

其中 `<ACCOUNT_ADDRESS>` 是您密钥对的公钥或收件人的公钥。

#### 转账测试的完整示例

```bash
$ solana-keygen new --outfile my_solana_wallet.json   # 创建第一个文件系统钱包
产生新的密钥对
为了增加安全性，输入一个密码(空白表示不设置密码)：
将新密钥对写入 my_solana_wallet.json
==========================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK                          # 第一个钱包的地址
==========================================================================
保存恢复密钥对的助记词：
width enhance concert vacant ketchup eternal spy craft spy guard tag punch    # 如果这是一个真实的钱包，不要将这次单词分享到网络上！
==========================================================================

$ solana airdrop 10 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://api.devnet.solana.com  # 空投 10 个 SOL 到我的钱包地址/公钥
正在从 35.233.193.70:9900 请求 10 SOL
10 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://api.devnet.solana.com # 检查钱包余额
10 SOL

$ solana-keygen new --no-outfile  # 创建第二个钱包即纸钱包
生成新的密钥对
为了增加安全性，输入一个密码(空白表示不设置密码)：
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # 这是第二个钱包即纸钱包的地址
=======================================================
保存助记词（用于恢复新秘钥对）：
clump panic cousin hurt coast charge engage fall eager urge win love  # 如果这是一个真实的钱包，切记不要将这次单词分享到网络上！
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 5 --url https://api.devnet.solana.com --fee-payer my_solana_wallet.json  # 发送代币到纸钱包的公钥地址
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # 该笔交易的签名

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://api.devnet.solana.com
4.999995 SOL  # 由于需要 0.000005 SOL 的交易费用，发送金额要稍微小于 5 SOL

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://api.devnet.solana.com
5 SOL  # 第二个钱包现在已经接收到第一个钱包发送的 5 SOL

```

## 接收代币

首先您需要一个地址让别人来发送代币。 在 Solana 区块链，钱包地址就是密钥对的公钥。 生成密钥对的方法有好几种。 这些方法取决于您选择如何存储密钥对。 密钥对存储在钱包里。 在接收代币之前，您需要通过 [来创建一个钱包](../wallet-guide/cli.md) 完成该步骤后，您就能获得每个密钥对生成的公钥。 公钥是一个 base58 字符的长字节。 其长度从 32 到 44 个字符不等。

## 发送代币

如果您已经持有 SOL 并想要向其他人发送代币，您将需要密钥对的路径， 他们的 base58 编码公钥和准备发送的代币。 上述条件准备好了以后，您可以使用 `solana transfer` 命令来发送代币：

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

使用 `solana balance` 确认余额已经更新：

```bash
solana balance <ACCOUNT_ADDRESS>
```
