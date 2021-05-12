---
title: トークンを送受信しよう
---

このページでは、[ペーパーウォレット](../wallet-guide/paper-wallet.md)、[ファイルシステムウォレット](../wallet-guide/file-system-wallet.md)、[ハードウェアウォレット](../wallet-guide/hardware-wallets.md)などのコマンドラインウォレットを使って、コマンドラインツールで SOL トークンを送受信する方法を説明します。 始める前に、ウォレットを作成し、そのアドレス(pubkey) と署名キーペアへのアクセス権を持っていることを確認してください。 異なるウォレットタイプのキーペアを入力するための [](../cli/conventions.md#keypair-conventions) の規則をチェックしてください。

## ウォレットを試そう

公開キーを他の人と共有する前に まず、キーが有効で、対応する秘密キーを保持していることを確認してください。

この例では、最初のウォレットに加えて 2 番目のウォレットを作成します。 そしてトークンをそこに転送します。 これにより、選択したウォレットタイプのトークンを送金できることが確認されます。

このテスト例では、開発者用の Testnet を使用します。 Devnet で発行されたトークンには \*\*\*\* の値がないので、それらを失っても心配しないでください。

#### 開始するにはトークンをいくつかドロップしてください

まず、 _airdrop_ 自身が devnet でトークンをプレイします。

```bash
solana airdrop 10 <RECIPIENT_ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

テキスト `<RECIPIENT_ACCOUNT_ADDRESS>` をベース 58 でエンコードされた公開キー/ウォレットアドレスに置き換えます。

#### 残高を確認しよう

アカウントの残高を確認してエアドロップが成功したことを確認します。 `10 SOL` を出力する必要があります。

```bash
solana balance <ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

#### 2 番目のウォレットアドレスを作成しよう

トークンを受け取るには新しいアドレスが必要です。 2 番目のキーペアを作成し、その公開キーを記録します:

```bash
solana-keygen new --no-passphrase -no-outfile
```

出力にはテキスト `pubkey:` の後にアドレスが含まれます。 アドレスをコピーしてください。 次のステップでそれを使用します。

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

また、[ペーパー](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses)、[ファイルシステム](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses)、[ハードウェア](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet)のいずれかによる任意の 2 つ目(またはそれ以上)のウォレットを作成することができます。

#### 最初のウォレットから 2 番目のアドレスにトークンを転送します

次に、それらを転送することによってエアドロップされたトークンを所有していることを証明します。 Solana クラスターは、トランザクション内の送信者の公開鍵に対応する秘密鍵ペアを使用してトランザクションに署名した場合にのみ、転送を受け付けます。

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 5 -url https://devnet.solana.com --fee-payer <KEYPAIR>
```

ここで、<`<KEYPAIR>`を 1 つ目のウォレットのキーペアへのパスに置き換え、`<RECIPIENT_ACCOUNT_ADDRESS>`を 2 つ目のウォレットのアドレスに置き換えます。

アップデートされた残高を `solana balance`で確認します。

```bash
solana balance <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

`<ACCOUNT_ADDRESS>` がキーペアの公開キーか、 受信者の公開キーのいずれかです。

#### テスト転送の完全な例

```bash
$ solana-keygen new --outfile my_solana_wallet.json   # Creating my first wallet, a file system wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
Wrote new keypair to my_solana_wallet.json
==========================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK                          # Here is the address of the first wallet
==========================================================================
Save this seed phrase to recover your new keypair:
width enhance concert vacant ketchup eternal spy craft spy guard tag punch    # If this was a real wallet, never share these words on the internet like this!
==========================================================================

$ solana airdrop 10 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 10 SOL to my wallet's address/pubkey
Requesting airdrop of 10 SOL from 35.233.193.70:9900
10 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
10 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # Here is the address of the second, paper, wallet.
============================================================================
新しいキーペアを回復するために、シードフレーズを保存してください。
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 5 --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
4.999995 SOL  # The sending account has slightly less than 5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://devnet.solana.com
5 SOL  # The second wallet has now received the 5 SOL transfer from the first wallet

```

## トークンの受信しよう

トークンを受け取るには、トークンを送信するためのアドレスが必要になります。 Solana では、ウォレットアドレスはキーペアの公開キーとなります。 キーペアを生成するためのさまざまな技術があります。 選択方法は、キーペアを保存する方法によって異なります。 キーペアはウォレットに保存されます。 トークンを受け取る前に[ウォレットを](../wallet-guide/cli.md)作成する必要があります。 完了後に、生成した各キーペアに公開キーが必要です。 公開キーは base58 であり、長い文字列です。 長さは 32 字から 44 字まで異なります。

## トークンを送信しよう。

すでに SOL を保有していて、誰かにトークンを送りたい場合は、自分のキーペアへのパス、相手の base58 でエンコードされた公開キー、そして転送するトークンの数が必要になります。 収集後にトークンを `solana transfer` コマンドで転送できます。

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

アップデートされた残高を `solana balance`で確認しよう。

```bash
ソラナ残高 <ACCOUNT_ADDRESS>
```
