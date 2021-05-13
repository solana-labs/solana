---
title: Ledger Nano
---

本网页展示了如何通过 Ledger Nano S 或 Nano X 来使用命令行工具与 Solana 交互。  其他解决方案与 Solana 与 Nano 的交互， [请点击这里](../ledger-live.md#interact-with-the-solana-network)

## 开始前的准备

- [使用 Solana 应用设置 Nano](../ledger-live.md)
- [安装 Solana 命令行工具](../../cli/install-solana-cli-tools.md)

## 通过 Solana CLI 使用 Ledger Nano

1. 确保 Ledger Live 应用程序已关闭
2. 将您的 Nano 插入计算机 USB 端口
3. 输入 pin 码并在 Nano 上启动 Solana 应用
4. 确保屏幕显示“应用程序已准备好”

### 查看您的钱包 ID

在您的计算机运行：

```bash
solana-keygen pubkey usb://ledger
```

该步骤确认您的 Ledger 设备已经连接正确，并且能够与 Solana CLI 正常交互。 该命令返回你 Ledger 设备唯一的_钱包 ID_. 当有多台 Nano 设备连接到同一台计算机时， 您可以通过钱包 ID 来指定想使用的 Ledger 硬件钱包。 如果您的电脑只使用一个 Nano 设备，那么就无需指明钱包 ID。 关于通过钱包 ID 使用特定 Ledger 的信息，请参阅[管理多个硬件钱包](#manage-multiple-hardware-wallets)。

### 查看您的钱包地址

您的 Nano 支持任意数量的有效钱包地址和签名者。 要查看任何地址，请使用前面所说的 `solana-keygen pubkey` 命令，然后接上一个有效的 [密钥对URL](../hardware-wallets.md#specify-a-keypair-url)。

如果需要在自己帐户之间的传输代币，您可以使用多个钱包地址。或在设备上对某一个抵押账号使用不同的键对作为签名授权。

以下所有的命令将显示不同的地址，关联到给定的密钥对路径。 来试一下吧！

```bash
solana-keygen pubkey usb://ledger
solana-keygen pubkey usb://ledger?key=0
solana-keygen pubkey usb://ledger?key=1
solana-keygen pubkey usb://ledger?key=2
```

* 注意：密钥对 URL 参数在 **zsh** &nbsp;[更多解决方案](#troubleshooting)中将被忽视。

您也可以在 `key=` 后面输入其他的数值。 这些命令显示的任何地址都是有效的 Solana 钱包地址。 与每个地址相关联的隐私信息都安全地存储在 Nano 上，并对该地址的交易进行签名。 只需给你生成的任何地址的密钥对 URL 进行备注，就可以用来接收代币。

如果您只计划设备上使用一个地址/密钥对， 那么容易记住的一个路径可能是 `key=0` 的地址。 通过该命令查看它的地址：

```bash
solana-keygen pubkey usb://ledger?key=0
```

现在，你已经有一个(或多个) 钱包地址，你可以公开分享其中的任何一个地址作为代币接收地址，并且使用关联的密钥对 URL 作为该地址发起交易的签名人。

### 查看钱包余额

无论是哪个钱包，您都可以通过 `solana balance` 命令来查看帐户余额：

```bash
solana balance SOME_WALLET_ADDRESS
```

例如，如果您的地址是 `7cvkjYAkUYs4W8XcXscca7cBrEGFeSUjeZmKoNBvEwyri`，那么可以输入以下命令查看余额：

```bash
solana balance 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri
```

您也可以在[Explorer](https://explorer.solana.com/accounts)查看任何账户地址的余额，在网页浏览器中将地址粘贴到搜索框来查看余额。

注意：任何余额为 0 SOL的地址（例如您在 Ledger 新创建的地址），将在浏览器器中显示“未找到”。 Solana 对空账户和不存在账户的处理是一样的。 当您的帐户地址中有一些 SOL 代币的时候才会正确显示。

### 从 Nano 发送 SOL

您需要使用该设备来签署交易，完成从 Nano 地址发送代币（通过生成钱包地址的相同密钥对 URL）。 请确保您的 Nano 已插入电脑，通过 PIN 解锁，并且 Ledger Live 处于未运行状态， 同时 Solana 应用在设备中打开，显示“应用已准备就绪”。

`solana transfer` 命令用于指定代币发送地址和数量，通过 `--keypair` 参数来指定发送代币的密钥对（签署交易），同时对应地址的余额将减少。

```bash
solana transfer RECIPIENT_ADDRESS AMOUNT --keypair KEYPAIR_URL_OF_SENDER
```

下面是一个完整的实例。 首先，通过某个密钥对 URL 中查看一个地址。 然后检查该地址的余额。 最后，输入一笔交易来发送 `1` SOL到接收地址 `7cvkjYAkUYs4W8XcXscca7cBrEGFeSUjeZmKoNBvEwyri`。 按下回车键传输命令时，您将看到在 Ledger 设备批准交易细节的提示。 在设备上通过左右键查看交易细节。 如果信息正确，请同时按下"允许"界面的两个按钮，否则请在"拒绝"界面按下这两个按钮。

```bash
~$ solana-keygen pubkey usb://ledger?key=42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://ledger?key=42

等待您在 Ledger 硬件钱包确认 usb://ledger/2JT2Xvy6T8hSmT8g6WdeDbHUgoeGdj6bE2VueCZUJmyN
✅ 已允许

签名：kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

在设备批准交易后，应用界面会显示交易签名，您需要等待最大的确认数量(32) 才能返回。 这个过程只需要几秒钟，然后交易就能在 Solana 网络确认。 您可以到[Explorer](https://explorer.solana.com/transactions)交易栏中粘贴该交易签名，来查看这笔交易或任何其他交易的详细信息。

## 进阶操作

### 管理多个硬件钱包

有时候通过多个硬件钱包对交易进行签名非常有用。 使用多个钱包签名需要 _完全合格的密钥对 URL_。 当 URL 不完全合格时，Solana CLI 将提示所有已连接硬件钱包的完全合格的 URL，并要求您选择每个签名使用哪个钱包。

您可以使用界面交互提示而不是使用 Solana CLI `交易栏` 命令来生成完全合格的 URL。 例如，试着将 Nano 连接到 USB，输入 PIN 码解锁，并运行以下命令：

```text
solana resolve-signer usb://ledger?key=0/0
```

您将看到类似这样的输出：

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

但 `BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` 是您的 `WALLET_ID`.

当 URL 完全合格时，您可以连接多个硬件钱包到同一台计算机，并独立识别其中任何一个私钥对。 除了 `<KEYPAIR>`，您可以在任何 `solana` 命令行的地方使用 `resolve-signer` 命令的输出去解决给定签名的解析路径问题。

## 疑难解答

### Zsh 忽略密钥对 URL 参数

Zsh 中问题标记字符是特殊字符。 如果您无需使用该功能，请在你的 `~/.zshrc` 中添加以下文本，将其作为的正常字符处理：

```bash
unsetopt nomatch
```

然后重启您的 shell 窗口，或者运行 `~/.zshrc`：

```bash
source ~/.zshrc
```

如果不想禁用 zsh 对问题标记字符的特殊处理，您可以在密钥对 URL 中使用反斜杠专门禁用它。 例如：

```bash
solana-keygen pubkey usb://ledger\?key=0
```

## 客服支持

查看 [钱包支持页面](../support.md) 获取帮助。

阅读更多关于 [发送和接收代币](../../cli/transfer-tokens.md) 和[委托质押](../../cli/delegate-stake.md)的信息。 您可以在任何接受 `<KEYPAIR>` 选项或参的地方使用 Ledger 密钥对 URL。
