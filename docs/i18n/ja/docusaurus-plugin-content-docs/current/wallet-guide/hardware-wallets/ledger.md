---
title: Ledger Nano
---

このドキュメントでは、Ledger Live ソフトウェアを使用して Ledger Nano S または Ledger Nano X をセットアップする方法を説明します。 Nano で Solana とやりとりするための他の解決策を見るには、 [ここをクリックしてください](../ledger-live.md#interact-with-the-solana-network)。

## はじめに

- [Solana アプリで Nano を設定します](../ledger-live.md)
- [Solana コマンドラインツールをインストールします。](../../cli/install-solana-cli-tools.md)

## Solana CLI で Ledger Nano を使用します。

1. Ledger Live アプリケーションが閉じていることを確認してください。
2. Nano をコンピューターの USB ポートに差し込みます。
3. ピンコードを入力し、Nano で Solana アプリを開始します
4. 画面に"Application is ready"と表示されることを確認します。

### ウォレット ID を確認してください。

お使いのコンピュータで以下を実行してください。

```bash
solana-keygen pubkey usb://ledger
```

これにより、Ledger デバイスが正しく接続されていることを確認し、正しい状態で Solana CLI と対話します。 このコマンドは Ledger 固有の *ウォレット ID*を返します。 同じコンピュータに複数の Nano デバイスを接続している場合はウォレット ID を使って、どの Ledger ハードウェアウォレットを使うかを指定することができます。 単一の Nano を使用する予定の場合は、ウォレット ID を含める必要はありません。 特定の Ledger を使用するためにウォレット ID を使用すること の詳細については、 [複数のハードウェアウォレットを管理する](#manage-multiple-hardware-wallets) を参照してください。

### ウォレットのアドレスを確認してください。

Nano は任意の数の有効なウォレットアドレスと署名者をサポートしています。 アドレスを表示するには、以下に示すように、 `solana-keygen pubkey` コマンドを使用します。 この後に有効な [キーペア URL](../hardware-wallets.md#specify-a-keypair-url)が続きます。

複数のウォレットアドレスは、トークンを自分のアカウント間で異なる目的のために移動させたい場合に便利です。 複数のウォレットアドレスは、自分のアカウント間でトークンを別の目的で転送したり、デバイス上の異なるキーペアをデバイスの異なるキーペアをステーキングアカウントの署名機関として使用する場合などに便利です。

以下のすべてのコマンドは、与えられたキーペアパス に関連付けられた異なるアドレスを表示します。 試してみてください！

```bash
solana-keygen pubkey usb://ledger
solana-keygen pubkey usb://ledger?key=0
solana-keygen pubkey usb://ledger?key=1
solana-keygen pubkey usb://ledger?key=2
```

- 注意: **zsh** &nbsp;[では、キーペアの URL パラメータは無視されます。詳細についてはトラブルシューティングを参照してください。](#troubleshooting)

`key=` 以降の数値にも他の値を使用できます。 これらのコマンドで表示されるアドレスは、有効な Solana ウォレット アドレスです。 各アドレスに関連付けられたプライベート部分は、Nano に安全に 保存され、このアドレスからの取引に署名するために使用されます。 トークンの受信に使用するアドレスを 取得するために使用したキーペア URL をメモしてください。

お使いのデバイスで単一のアドレス/キーペアを使用することを計画している場合。 良い 覚えやすいパスは、 `key=0` のアドレスを使うことかもしれません。 このアドレス の表示方法:

```bash
solana-keygen pubkey usb://ledger?key=0
```

これで、ウォレットアドレス(または複数のアドレス) ができました。 そのアドレスを公開して受信アドレスとして利用したり、そのアドレスからの取引の署名者として そのアドレスからの取引の署名者として、関連するキーペア URL を使用することができます。

### バランスを確認してください。

どのウォレットを使用しているかに関係なく、アカウントの残高を表示するには、 `solana balance` コマンドを使用します。

```bash
solana balance SOME_WALLET_ADDRESS
```

例えば、あなたのアドレスが `7cvkjYAKUYs4W8XcXsca7cBrEGFeZmKoNBvEwyri`の場合、 次のコマンドを入力してバランスを確認します。

```bash
solana balance 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri
```

また、[エクスプローラー](https://explorer.solana.com/accounts)の"アカウント"タブで任意のアカウントアドレスの残高を確認し、ボックスにアドレスを貼り付けると、ウェブブラウザで残高を確認することができます。

注: Ledger で新しく作成されたものなど、残高が 0SOL のすべてのアドレス はエクスプローラに "Not Found" と表示されます。 空のアカウントと存在しないアカウントは Solana での扱いは同じです。 アカウントのアドレスに SOL が含まれている場合、これは変更されます。

### Nano から SOL を送信

Nano で管理されているアドレスからトークンを送信するには、デバイスを使って アドレスの取得に使用したのと同じキーペアの URL を使用して、デバイスでトランザクションに署名する必要があります。 これを行うには、あなたの Nano がプラグインされていることを確認してください。 PIN でロックが解除されていること、Ledger Live が起動していないこと、Solana アプリがデバイス上で開いていて"Application is Ready"と表示されていることを確認します。

`solana transfer`コマンドは、どのアドレスにトークンを送るか、何トークンを送るかを指定するために使用され、`--keypair`引数を使用して、どのキーペアがトークンを送るか、どのキーペアがトランザクションに署名するかを指定し、関連するアドレスからの残高が減少します。

```bash
solana transfer RECIPENT_ADDRESS AMOUNT --keypair KEYPAIR_URL_OF_SENDER
```

以下に、完全な例を示します。 まず、アドレスは特定のキーペアの URL で表示されます。 第二に、アドレスの残高が確認されます。 最後に、送金トランザクション が `1` `7cvkjYAKUYs4W8XcXsca7cBrEGFeZmKoNBvEwyri` を送信するために入力されます。 Enter キーを押して転送コマンドを実行すると、Ledger デバイスの トランザクション詳細を承認するよう求められます。 デバイスでは、右と左のボタンを使用してトランザクションの詳細を確認します。 正しく見える場合は、"Approve"画面の両方のボタンを クリックし、それ以外の場合は"Rejecet"画面の両方のボタンを押します。

```bash
~$ solana-keygen pubkey usb://ledger?key=42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://ledger?key=42
Waiting for your approval on Ledger hardware wallet usb://ledger/2JT2Xvy6T8hSmT8g6WdeDbHUgoeGdj6bE2VueCZUJmyN
✅ Approved

Signature: kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

お使いのデバイスでトランザクションを承認すると、プログラムはトランザクションの署名を表示します。 トランザクションの署名を表示し、最大確認数(32) を待ってから戻ります。 これには数秒しかかからず、トランザクションは Solana ネットワーク上で確定されます。 この取引やその他の取引の詳細は、[エクスプローラー](https://explorer.solana.com/transactions)の"取引"タブを開き、取引署名を貼り付けることで確認できます。

## 高度な操作

### 複数のハードウェアウォレットを管理

複数のハードウェアウォレットからのキーでトランザクションに署名することは時々便利です。 複数のウォレットでサインインするには、 *完全修飾キーペア URL*が必要です。 URL が完全に修飾されていない場合、Solana CLI は接続されているすべてのハードウェアウォレットの完全に修飾された URL を尋ねます。 接続されているすべてのハードウェアウォレットの完全修飾 URL が表示され、各署名に使用するウォレットを選択するように求められます。

対話型プロンプトを使用する代わりに、Solana CLI の`resolve-signer`コマンドを使用して完全修飾 URL を生成することができます。 例えば、以下のようにしてみてください。 Nano を USB に接続し、ピンでロックを解除して、以下のコマンドを実行してみてください。 次のコマンドを実行します。

```text
solana resolve-signer usb://ledger?key=0/0
```

次のような出力が表示されます：

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

`BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` はあなたの `WALLET_ID` です。

完全修飾 URL を使用すると、複数のハードウェアウォレットを同じコンピュータに 接続し、キーペアを特定することができます。 `resolve-signer`コマンドの出力を、`solana`コマンドが`<KEYPAIR>`エントリを要求する場所で使用して、その解決されたパスをそのトランザクションの署名者として使用します。

## トラブルシューティング

### "Keypair URL" パラメーターは "zsh" では無視されます。

疑問符文字は "zsh" の特殊文字です。 それが 機能ではない場合は、 `~/.zshrc` に次の行を追加して、 通常の文字として扱います。

```bash
unsetopt nomatch
```

次にシェルウィンドウを再起動するか、 `~/.zshrc` を実行します。

```bash
source ~/.zshrc
```

クエスチョンマーク文字に対する"zsh"の文字の特別な処理を無効にしたくない場合は、キーペアの URL にバックスラッシュを入れて明示的に無効にすることができます。 例:

```bash
solana-keygen pubkey usb://ledger?key=0
```

## サポート

[ウォレットサポートページ](../support.md) を参考にしてください。

[トークンの送受信](../../cli/transfer-tokens.md) と [ステーキングの委任](../../cli/delegate-stake.md) についての詳細はこちらをご覧ください。 Ledger キーペアの URL は、`<KEYPAIR>`を受け付けるオプションや引数であれば、どこでも使用できます。
