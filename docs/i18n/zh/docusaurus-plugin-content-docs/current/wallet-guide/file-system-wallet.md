---
title: 文件系统钱包
---

本文档介绍了如何使用 Solana CLI 工具创建和使用文件系统钱包。 文件系统钱包的形式为计算机文件系统上的未加密密钥对文件。

> 文件系统钱包是存储 SOL 代币 **安全性最差**的方法。 我们 **不建议** 在文件系统钱包中存储大量代币。

## 准备工作

确保您 [已安装Solana命令行工具](../cli/install-solana-cli-tools.md)

## 生成文件系统钱包密钥对

使用 Solana 的命令行工具 `solana-keygen` 来生成密钥对文件。 例如，从命令行 shell 运行下面的命令：

```bash
mkdir ~/my-solana-wallet
solana-keygen new --outfile ~/my-solana-wallet/my-keypair.json
```

此文件包含您的 **未加密的** 密钥对。 实际上，即使您指定了密码，该密码也适用于恢复种子短语，而不适用于文件。 请不要分享这个文件给其他人。 有权访问此文件的任何人都可以转移该钱包地址的所有代币。 相反，您应该仅分享钱包的公共密钥。 要显示其公钥，请运行：

```bash
solana-keygen pubkey ~/my-solana-wallet/my-keypair.json
```

它将输出一个字符串，例如：

```text
ErRr1caKzK8L8nn4xmEWtimYRiTCAZXjBtVphuZ5vMKy
```

这是 `~/my-solana-wallet/my-keypair.json` 中对应的公钥。 密钥对文件的公钥即为您的 _钱包地址_。

## 使用密钥对文件验证您的地址

如需验证您某个给定地址的私钥，请使用 `solana-keygen verify` 命令：

```bash
solana-keygen verify <PUBKEY> ~/my-solana-wallet/my-keypair.json
```

其中 `<PUBKEY>` 替换为你的钱包地址。 如果给定的地址与密钥对文件中的地址匹配，则命令将输出“成功”，否则输出“失败”。

## 创建多个文件系统钱包地址

您可以根据需要创建任意数量的钱包地址。 只需重新运行 [生成一个文件系统钱包](#generate-a-file-system-wallet-keypair) 命令，并确保传入 `--outfile` 参数来使用新文件名或路径。 如果需要在自己的帐户之间转移代币，多个钱包地址可能会很有用。
