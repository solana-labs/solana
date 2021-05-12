---
title: Ledger Nano S and Nano X
---

このドキュメントでは、[Ledger Nano S ](https://shop.ledger.com/products/ledger-nano-s)または [Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) を[ Ledger Live](https://www.ledger.com/ledger-live) ソフトウェアでセットアップする方法を説明します。

以下のセットアップ手順が完了し、Solana アプリが Nano デバイスにインストールされると、ユーザーは[Nano を使って Solana ネットワークと対話する方法](#interact-with-the-solana-network)をいくつか選ぶことができます。

## はじめに

- [Nano S](https://shop.ledger.com/products/ledger-nano-s)または[Nano X](https://shop.ledger.com/pages/ledger-nano-x)を Ledger 社に注文します。
- パッケージに同梱されているデバイスのセットアップ手順、または[Ledger](https://www.ledger.com/start/)のスタートページに従ってください。
- Install [Ledger Live desktop software](https://www.ledger.com/ledger-live/)
  - すでに Ledger Live がインストールされている場合は、最新のファームウェアやアプリのアップデートが可能な Ledger Live の最新版にアップデートしてください。
- Nano をコンピューターに接続し、画面上の指示に従ってください。
- 新しい Nano のファームウェアをアップデートします。 これは、最新版の Solana アプリを確実にインストールするために必要です。
  - [Update Nano S Firmware](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Update Nano X Firmware](https://support.ledger.com/hc/en-us/articles/360013349800)

## Nano に Solana アプリをインストールしよう

- Ledger Live を開きます
- アプリの左ページにある"マネージャ"をクリックし、アプリカタログで"Solana"を検索し、"インストール"をクリックします。
  - デバイスが USB で接続されており、PIN でロックが解除されていることを確認してください。
- Nano では Solana App のインストールを確認するプロンプトが表示されることがあります。
- "Solana "は Ledger Live Manager で "Installed "と表示されます。

## ソラナアプリの最新版へのアップグレード

最新の機能をご利用いただくために、旧バージョンの Solana App をお使いの方は、以下の手順でバージョン`v1.0.1`にアップグレードしてください。

- Ledger Live のバージョンが"2.10.0"以降であることを確認してください。
  - お使いの Ledger Live のバージョンを確認するには、右上の"Settings"ボタンをクリックし、"About"をクリックします。 新しいバージョンの Ledger Live が利用可能な場合は、Ledger Live を初めて開いたときに、アップグレードを促すバナーが表示されます。
- Nano のファームウェアのアップデート
  - [Update Nano S Firmware](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Update Nano X Firmware](https://support.ledger.com/hc/en-us/articles/360013349800)
- ファームウェアのアップデートが成功すると、Solana アプリは自動的に最新版が再インストールされるはずです。

## Solana ネットワークとの交流

ユーザーは、以下のいずれかのオプションを使用して、自分の Nano を使って Solana と対話することができます。

- [SolFlare.com](https://solflare.com/)は、Solana 専用に構築された非親告罪のウェブウォレットで、Ledger デバイスによる基本的な送金やステーキング操作をサポートします。 [Nano と SolFlare の組み合わせ](solflare.md)については、こちらをご覧ください。

- 開発者や上級ユーザーは、[Solana コマンドラインツールで Nano を使用する](hardware-wallets/ledger.md)ことができます。 新しいウォレットの機能は、ほとんどの場合、サードパーティのウォレットでサポートされる前に、ネイティブのコマンドラインツールでサポートされます。

## 既知の問題

- Nano X では Windows Os を使用するウェブウォレットに接続できないことがあります。 これは、WebUSB を使用するブラウザベースのウォレットに影響を与える可能性があります。 Ledger チームはこの問題の解決に取り組んでいます。

## サポート

[ウォレットサポートページ](support.md)を参考にしてください。
