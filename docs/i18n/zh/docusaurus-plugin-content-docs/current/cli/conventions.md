---
title: 使用 Solana CLI 命令
---

在运行 Solana CLI 命令之前，让我们先来熟悉一下所有的命令。 首先，Solana CLI 实际上是你可能会用到操作的命令集合。 您可以通过运行以下操作查看所有可能命令的列表：

```bash
solana --help
```

需要研究特定的命令，请运行：

```bash
solana <COMMAND> --help
```

您可以将文本 `<COMMAND>` 替换为你想了解的命令名称。

命令使用信息通常包含诸如 `<AMOUNT>`、`<ACCOUNT_ADDRESS>` 或 `<KEYPAIR>` 等字段。 每个字段都是 _type_ 文本的占位符，您可以使用它执行命令。 例如，您可以将 `<AMOUNT>` 替换为诸如 `42` 或 `100.42` 等数字。 您可以将 `<ACCOUNT_ADDRESS>` 替换为公钥的 base58 编码，例如 `9grmKMwTiZwUHSExjtbFzHLPTdWoXgcg1bZkhvwTrTww`。

## 密钥对协议

许多使用 CLI 工具的命令需要一个 `<KEYPAIR>` 值。 密钥对应使用的值取决于您创建的 [命令行钱包的类型](../wallet-guide/cli.md)。

例如，显示钱包地址(也称为密钥)，CLI 帮助文档显示：

```bash
solana-keygen pubkey <KEYPAIR>
```

下面我们来展示根据钱包类型来解决应该在 `<KEYPAIR>` 中插入什么的问题。

#### 纸钱包

在纸质钱包中，密钥对源自助记词和你在钱包创建时输入的可选密码。 若要让纸钱包密钥在任意地方显示 `<KEYPAIR>` 文本在示例或帮助文档中，请输入单词 `ASK`，然后程序会强制您在运行命令时输入种子单词。

显示纸钱包的钱包地址：

```bash
solana-keygen pubkey
```

#### 文件系统钱包

有了一个文件系统钱包，密钥对就会存储在您的计算机上的文件中。 将 `<KEYPAIR>` 替换为对密钥文件的完整文件路径。

例如，如果文件系统密钥对文件位置是 `/home/solana/my_wallet.json`，请输入来显示地址：

```bash
solana-keygen pubkey /home/solana/my_wallet.json
```

#### 硬软件钱包

如果您选择了硬件钱包，请使用您的 [密钥对链接](../wallet-guide/hardware-wallets.md#specify-a-hardware-wallet-key)，比如 `blap://ledger?key=0`。

```bash
solana-keygen pubkey usb://ledger?key=0
```
