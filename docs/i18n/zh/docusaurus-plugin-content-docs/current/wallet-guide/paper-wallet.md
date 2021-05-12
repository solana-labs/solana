---
title: 纸钱包
---

本文档描述了如何使用 Solana CLI 工具创建和使用纸钱包。

> 在此，我们不提供如何 _安全地_ 创建或管理纸钱包的建议。 请仔细研究安全相关的问题。

## 概述

Solana 提供了一个密钥生成工具，可以从符合 BIP39 规范的助记词中获取密钥。 用于运行验证节点和质押代币的 Solana CLI 命令均支持通过助记词输入密钥对。

要了解更多关于 BIP39 标准的信息，请访问 [这里](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki) 查看比特币 BIPs Github 代码库。

## 纸钱包使用方法

无需将密钥对保存到计算机上的磁盘即可运行 Solana 命令。 如果将私钥写入磁盘可能会遇到安全问题，那么这个指南对你就非常有帮助了。

> 即使使用这个安全的输入方法，私钥仍有可能通过未加密的内存交换被写入到磁盘中。 避免这种情况的发生是用户的责任。

## 准备工作

- [安装 Solana 命令行工具](../cli/install-solana-cli-tools.md)

### 检查您的安装

运行 `solana-keygen` 确保已正确安装：

```bash
solana-keygen --version
```

## 创建一个纸钱包

使用 `solana-keygen` 工具可以生成新的助记词，并且从现有助记词和 (可选) 密码中生成一个密钥对。 助记词和密码可以作为纸钱包一起使用。 只要您将助记词和密码安全地存储起来，就可以使用它们来访问您的帐户。

> 如需了解更多关于助记词工作原理的信息，请参阅 [比特币百科网页](https://en.bitcoin.it/wiki/Seed_phrase)。

### 生成助记词

使用 `solana-keygen new` 命令生成新的密钥对。 该命令将生成一个随机的助记词，要求您输入一个可选的密码，然后将显示派生的公钥和纸钱包生成的助记词。

复制助记词以后，您可以使用 [公钥派生](#public-key-derivation) 说明来验证操作没有任何错误。

```bash
solana-keygen new --no-outfile
```

> 如果 `--no-outfile` 标志显示为 **omitted**，那么默认行为是将密钥写入到 `~/.config/solana/id.json`，最终产生一个 [文件系统钱包](file-system-wallet.md)

此命令的输出将显示下面的这一行：

```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

`pubkey: ` 后面显示的值即为您的 _钱包地址_。

**请注意：** 在使用纸钱包和文件系统钱包时，“pubkey”和“钱包地址”有时会互换使用。

> 为了增加安全性，请使用 `--word-count` 参数增加助记词的数量

完整的使用详细信息请运行：

```bash
solana-keygen new --help
```

### 公钥派生

如果您选择使用公钥，则可以从助记词和密码派生公钥。 This is useful for using an offline-generated seed phrase to derive a valid public key. The `solana-keygen pubkey` command will walk you through how to use your seed phrase (and a passphrase if you chose to use one) as a signer with the solana command-line tools using the `ask` uri scheme.

```bash
solana-keygen pubkey prompt://
```

> 请注意，对于相同的助记词，您可能会使用不同的密码。 每个唯一的密码将产生不同的密钥对。

`solana-keygen` 工具与生成助记词的 BIP39 标准的英文单词列表是一样的。 如果您的助记词是通过另一个工具生成，您仍然可以使用 `solana-keygen` 命令，但需要通过 `--skip-seed-spoe-valide-` 参数并放弃验证。

```bash
solana-keygen pubkey prompt:// --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey prompt://` the console will display a string of base-58 character. This is the base _wallet address_ associated with your seed phrase.

> 复制派生地址到 USB 以便网络计算机使用

> 通常下一步是 [检查与公钥关联的帐户余额](#checking-account-balance)

完整是使用详细信息请运行：

```bash
solana-keygen pubkey --help
```

### Hierarchical Derivation

The solana-cli supports [BIP32](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki) and [BIP44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) hierarchical derivation of private keys from your seed phrase and passphrase by adding either the `?key=` query string or the `?full-path=` query string.

By default, `prompt:` will derive solana's base derivation path `m/44'/501'`. To derive a child key, supply the `?key=<ACCOUNT>/<CHANGE>` query string.

```bash
solana-keygen pubkey prompt://?key=0/1
```

To use a derivation path other than solana's standard BIP44, you can supply `?full-path=m/<PURPOSE>/<COIN_TYPE>/<ACCOUNT>/<CHANGE>`.

```bash
solana-keygen pubkey prompt://?full-path=m/44/2017/0/1
```

Because Solana uses Ed25519 keypairs, as per [SLIP-0010](https://github.com/satoshilabs/slips/blob/master/slip-0010.md) all derivation-path indexes will be promoted to hardened indexes -- eg. `?key=0'/0'`, `?full-path=m/44'/2017'/0'/1'` -- regardless of whether ticks are included in the query-string input.

## 验证密钥对

如需要验证您控制纸钱包地址的私钥，请使用 `solana-keygen verify` 命令：

```bash
solana-keygen verify <PUBKEY> prompt://
```

where `<PUBKEY>` is replaced with the wallet address and the keyword `prompt://` tells the command to prompt you for the keypair's seed phrase; `key` and `full-path` query-strings accepted. Note that for security reasons, your seed phrase will not be displayed as you type. After entering your seed phrase, the command will output "Success" if the given public key matches the keypair generated from your seed phrase, and "Failed" otherwise.

## 检查账户余额

检查账户余额仅需要某个账户的公钥。 要安全地从纸钱包产生公钥， 请按照在一台 [气隙计算机](<https://en.wikipedia.org/wiki/Air_gap_(networking)>) 进行 [公钥衍生](#public-key-derivation) 的说明。 然后公钥可以通过手动输入或 USB 传输一台网络设备。

接下来，配置 `solana` CLI 工具到 [连接一个特定集群](../cli/choose-a-cluster.md)：

```bash
solana config set --url <CLUSTER URL> # (例如 https://api.mainnet-beta.solana.com)
```

最后，如需检查余额，请运行以下命令：

```bash
solana balance <PUBKEY>
```

## 创建多个纸钱包地址

您可以根据需要创建任意数量的钱包地址。 只需重复运行 [生成助记词](#seed-phrase-generation) 或 [公钥衍生](#public-key-derivation)，就可以创建一个新地址。 如果需要在自己的帐户之间转移代币，多个钱包地址可能会很有用。

## 支持

请查看 [已支持钱包页面](support.md) 来获得帮助。
