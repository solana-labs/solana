---
title: Ledger Nano S and Nano X
---

このドキュメントでは、[Ledger Nano S ](https://shop.ledger.com/products/ledger-nano-s)または [Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) を[ Ledger Live](https://www.ledger.com/ledger-live) ソフトウェアでセットアップする方法を説明します。

以下のセットアップ手順が完了し、SolanaアプリがNanoデバイスにインストールされると、ユーザーは[Nanoを使ってSolanaネットワークと対話する方法](#interact-with-the-solana-network)をいくつか選ぶことができます。

## はじめに

- [Nano S](https://shop.ledger.com/products/ledger-nano-s)または[Nano X](https://shop.ledger.com/pages/ledger-nano-x)をLedger社に注文します。
- パッケージに同梱されているデバイスのセットアップ手順、または[Ledger](https://www.ledger.com/start/)のスタートページに従ってください。
- Install [Ledger Live desktop software](https://www.ledger.com/ledger-live/)
  - すでにLedger Liveがインストールされている場合は、最新のファームウェアやアプリのアップデートが可能なLedger Liveの最新版にアップデートしてください。
- Nanoをコンピューターに接続し、画面上の指示に従ってください。
- 新しいNanoのファームウェアをアップデートします。  これは、最新版のSolanaアプリを確実にインストールするために必要です。
  - [Update Nano S Firmware](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Update Nano X Firmware](https://support.ledger.com/hc/en-us/articles/360013349800)

## Nano に Solana アプリをインストールしよう

- Ledger Liveを開きます
- アプリの左ページにある"マネージャ"をクリックし、アプリカタログで"Solana"を検索し、"インストール"をクリックします。
  - デバイスがUSBで接続されており、PINでロックが解除されていることを確認してください。
- NanoではSolana Appのインストールを確認するプロンプトが表示されることがあります。
- "Solana "はLedger Live Managerで "Installed "と表示されます。

## ソラナアプリの最新版へのアップグレード

最新の機能をご利用いただくために、旧バージョンのSolana Appをお使いの方は、以下の手順でバージョン`v1.0.1`にアップグレードしてください。

- Ledger Liveのバージョンが"2.10.0"以降であることを確認してください。
  - お使いのLedger Liveのバージョンを確認するには、右上の"Settings"ボタンをクリックし、"About"をクリックします。  新しいバージョンのLedger Liveが利用可能な場合は、Ledger Liveを初めて開いたときに、アップグレードを促すバナーが表示されます。
- Nanoのファームウェアのアップデート
  - [Update Nano S Firmware](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Update Nano X Firmware](https://support.ledger.com/hc/en-us/articles/360013349800)
-  ファームウェアのアップデートが成功すると、Solanaアプリは自動的に最新版が再インストールされるはずです。

## Solanaネットワークとの交流

ユーザーは、以下のいずれかのオプションを使用して、自分のNanoを使ってSolanaと対話することができます。

- [SolFlare.com](https://solflare.com/)は、Solana専用に構築された非親告罪のウェブウォレットで、Ledgerデバイスによる基本的な送金やステーキング操作をサポートします。 [NanoとSolFlareの組み合わせ](solflare.md)については、こちらをご覧ください。

- 開発者や上級ユーザーは、[SolanaコマンドラインツールでNanoを使用する](hardware-wallets/ledger.md)ことができます。 新しいウォレットの機能は、ほとんどの場合、サードパーティのウォレットでサポートされる前に、ネイティブのコマンドラインツールでサポートされます。

## 既知の問題

- Nano XではWindows Osを使用するウェブウォレットに接続できないことがあります。 これは、WebUSBを使用するブラウザベースのウォレットに影響を与える可能性があります。 Ledgerチームはこの問題の解決に取り組んでいます。

## サポート

[ウォレットサポートページ](support.md)を参考にしてください。
