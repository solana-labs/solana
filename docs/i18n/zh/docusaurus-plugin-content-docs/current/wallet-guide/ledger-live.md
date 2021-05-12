---
title: Ledger Nano S 和 Nano X
---

本文档描述了如何设置 [Ledger Nano S](https://shop.ledger.com/products/ledger-nano-s)、[Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) 与 [Ledger Live](https://www.ledger.com/ledger-live) 软件。

一旦下面显示的设置步骤完成，Solana 应用程序将安装在您的 Nano 设备上，用户有几个方法来 [使用 Nano 来与 Solana 网络交互](#interact-with-the-solana-network)。

## 准备工作

- 首先需要从 Ledger 订购一个 [Nano S](https://shop.ledger.com/products/ledger-nano-s) 或 [Nano X](https://shop.ledger.com/pages/ledger-nano-x)。
- 按照快递中的使用说明或 [Ledger's Start page](https://www.ledger.com/start/) 来设置设备。
- 安装 [Ledger Live 桌面软件](https://www.ledger.com/ledger-live/)
  - 如果您已经安装了 Ledger Live，请更新到最新版本，该步骤将启用最新的固件和应用更新。
- 将您的 Nano 连接到电脑并按照屏幕的指示。
- 更新您的新 Nano设备 固件。  该步骤确保您安装了最新版本的 Solana 应用。
  - [更新 Nano S 固件](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [更新 Nano X 固件](https://support.ledger.com/hc/en-us/articles/360013349800)

## 在 Nano 上安装 Solana 应用

- 打开 Ledger Live
- 点击应用程序左边窗口的“管理”，在应用目录中搜索"Solana"，然后点击"安装"。
  - 请确保您的设备已通过 USB 接入并且通过 PIN 解锁。
- 您可以在 Nano 上看到确认安装 Solana App 的提示
- 在Ledger Live Manager 中，"Solana" 现在应该显示为“已安装”

## 升级到最新版本的 Solana App

为了确保您能够使用最新功能，如果您使用的是旧版本 Solana App，请按照以下步骤升级到 `v1.0.1`。

- 确保您的 Ledger Live 是 2.10.0 或更高版本。
  - 要检查您的 Ledger Live 版本，请单击右上角的“设置”按钮，然后单击“关于”。  如果有较新版本的 Ledger Live，那么当您首次打开 Ledger Live 时，应该会看到一个提示您进行升级的弹窗。
- 在您的 Nano 更新固件
  - [更新 Nano S 固件](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [更新 Nano X 固件](https://support.ledger.com/hc/en-us/articles/360013349800)
-  成功更新固件后，Solana 应用会自动重新安装最新版本的应用程序。

## 与 Solana 网络交互

用户可以使用以下任何选项来通过 Nano 与 Solana 进行交互：

- [SolFlare.com](https://solflare.com/) 一款是专门为 Solana 打造的非托管网络钱包，它支持使用 Ledger 设备进行的基本转账和质押操作。 请查阅 [结合 Nano 与 SolFlare 使用](solflare.md) 的指南。

- 开发者和高级用户可以 [结合使用 Nano与 Solana 命令行工具](hardware-wallets/ledger.md)。 在第三方钱包支持之前，本地命令行工具几乎总能够支持最新的钱包功能。

## 已知的问题

- 在 Windows 系统，Nano X 有时无法连接到网页钱包。 这可能会影响任何使用 WebUSB 的浏览器钱包。 Ledger 团队正在努力解决这个问题。

## 客服支持

请查看 [已支持钱包页面](support.md) 来获得帮助。
