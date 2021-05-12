---
title: 在Solana CLI上使用硬件钱包
---

签署任何一笔交易都需要私钥，但是在您的个人计算机或手机上存储私钥可能很容易被盗。 因此对密钥加入密码可以提高安全性，但是许多用户追求进一步的安全性，希望将私钥移动到称为 _硬件钱包_的独立物理设备上。 硬件钱包是一个小型的手持设备，它存储私钥并提供用于签署交易的界面。

Solana CLI对硬件钱包的支持非常充分。 在任何使用密钥对文件路径的地方(文档中用 `<KEYPAIR>` 表示)，都可以传入一个 _keypair URL_，该URL唯一地标识硬件钱包中的密钥对。

## 支持的硬件钱包

Solana CLI 支持以下硬件钱包：

- [Ledger Nano S 和 Ledger Nano X](hardware-wallets/ledger.md)

## 指定密钥对 URL

Solana 定义了密钥对URL格式，让与计算机相连的硬件钱包唯一定位任何 Solana 密钥对。

密钥对的 URL 有以下形式，其中方括号表示可选字段：

```text
usb://<MANUFACTURER>[/<WALLET_ID>][?key=<DERIVATION_PATH>]
```

`WALLET_ID` 是一个用于清除多个设备的全局唯一密钥。

`DERVIATION_PATH` 用于导航到您硬件钱包中的 Solana 密钥。 路径有表单 `<ACCOUNT>[/<CHANGE>]`，其中每一个 `ACOUNT` 和 `CHANGE` 都是正整数。

例如，Ledger 设备完全合格的 URL 可能是：

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

所有衍生路径都隐含了前缀 `44'/501'`， 其中表示路径遵循 [BIP44 规则](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)，并且任何派生的密钥都是 Solana 密钥(代币类型为 501)。 单引号表示一个“固化”衍生。 因为 Solana 采用 Ed25519 密钥对，所以所有衍生地址都是固化的，因此添加单引号是可选的而不是必须的。
