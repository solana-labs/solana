---
title: Solana CLI の使用方法
---

Solana CLIコマンドを実行する前に、すべてのコマンドに共通するいくつかの規約を確認しましょう。 まず、Solana CLIは、取りたいと思うアクションごとに異なるコマンドを集めたものです。 以下を実行することで、可能なすべてのコマンドのリストを見ることができます。

```bash
solana --help
```

特定のコマンドの使用方法を拡大するには、以下を実行します。

```bash
solana <COMMAND> --help
```

をクリックします。テキスト `<COMMAND>` を、 についてもっと学びたいコマンドの名前に置き換えます。

コマンドの使用状況メッセージには、通常、 `<AMOUNT>`、 `<ACCOUNT_ADDRESS>` または `<KEYPAIR>` などの単語が含まれます。 各単語は、コマンドを実行できる _テキストの_ タイプ のプレースホルダです。 たとえば、 `<AMOUNT>` を `42` や `100.42` などの数値に置き換えることができます。 `<ACCOUNT_ADDRESS>` を`9grmKMwTiZwUHSExjtbFzHLPTdWoXgcg1bZkhvwTrTw` のような公開鍵の base58 エンコーディングで置き換えることができます。

## キーペアの規則

CLI ツールを使用する多くのコマンドは、 `<KEYPAIR>` の値を必要とします。 キーペアに使用すべき値 は、 [コマンドラインウォレットの種類によって異なります。](../wallet-guide/cli.md).

たとえば、ウォレットのアドレス(キーペアの pubkey とも呼ばれます) を表示する方法は、CLI ヘルプドキュメントに表示されます。

```bash
solana-keygen pubkey <KEYPAIR>
```

以下では、ウォレットの種類に応じて、`<KEYPAIR>` に入れるべきものを解決する方法をご紹介します

#### ペーパーウォレット

ペーパーウォレットでは、ウォレットの作成時に入力したシードワードとオプションのパスフレーズからキーペアを安全に導き出します。 ペーパーウォレットを使うにはkeypair サンプルやヘルプドキュメントで ` <KEYPAIR> ` のテキストが表示されている場所であればどこでも、`"ASK"` という単語を入力しておけば、コマンドを実行したときにプログラムがシードワードの入力を促してくれます。

ペーパーウォレットのウォレットアドレスを表示するには:

```bash
solana-keygen pubkey ASK
```

#### ファイルシステムウォレット

ファイルシステムウォレットを使用すると、キーペアはコンピュータ上のファイルに保存されます。 `<KEYPAIR>` をキーペアファイルの完全なファイルパスに置き換えます。

たとえば、ファイルシステムのキーペアファイルの場所が`/home/solana/my_wallet.json`の場合、アドレスを表示します。

```bash
solana-keygen pubkey /home/solana/my_wallet.json
```

#### ハードウェアウォレット

ハードウェアウォレットを選択した場合は、 [キーペア URL](../wallet-guide/hardware-wallets.md#specify-a-hardware-wallet-key)、 などを使用してください `usb://ledger?key=0`

```bash
solana-keygen pubkey usb://ledger?key=0
```
