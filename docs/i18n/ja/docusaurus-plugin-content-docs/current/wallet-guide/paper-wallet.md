---
title: ペーパーウォレット
---

このドキュメントでは、Solana CLI ツールを使用してペーパーウォレットを作成して使用する方法について説明します。

> 当社は、ペーパーウォレットを*安全*に作成・管理する方法をアドバイスするつもりはありません。 セキュリティについては、よくお調べください。

## 概要

Solana は、"BIP39"準拠のシードフレーズからキーを派生するためのキー生成ツールを提供します。 バリデータとステーキングトークンを実行するための Solana CLI コマンド はシードフレーズを介してキーペア入力をサポートしています。

BIP39 規格の詳細については、[こちら](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki)から Bitcoin BIPs の Github リポジトリをご覧ください。

## ペーパーウォレット使用方法

Solana コマンドはマシン上のキーペアをディスクに保存することなく実行できます。 秘密キーのディスクへの書き込みを避けることがセキュリティ上の関心事であるならば、あなたは正しい場所に来たと言えるでしょう。

> このような安全な入力方法を採用していても、暗号化されていないメモリスワップによって秘密キーがディスクに書き込まれる可能性があります。 このような事態を防ぐのは、ユーザーの責任です。

## はじめに

- [Solana コマンドラインツールをインストール](../cli/install-solana-cli-tools.md)

### インストールを確認する

`solana-keygen` が以下を実行して正しくインストールされていることを確認してください。

```bash
solana-keygen --version
```

## ペーパーウォレットの作成

Using the `solana-keygen` tool, it is possible to generate new seed phrases as well as derive a keypair from an existing seed phrase and (optional) passphrase. "シードフレーズ"と"パスフレーズ"は、ペーパーウォレットとして一緒に使用できます。 "シードフレーズ"と"パスフレーズ"を安全に保管することでアカウントにアクセスすることができます。

> シードフレーズの仕組みの詳細については、こちらをご覧ください [Bitcoin Wiki ページ](https://en.bitcoin.it/wiki/Seed_phrase).

### シードフレーズの生成

新しいキーペアの生成は `solana-keygen new` コマンドを使用して行うことができます。 このコマンドは、ランダムなシードフレーズを生成し、オプションのパスフレーズを入力するように求め、その後、ペーパーウォレット用に派生した公開キーと生成されたシードフレーズを表示します。

シードフレーズをコピーした後に [public key derivation](#public-key-derivation) 命令を使用してエラーがなかったことを確認できます。

```bash
solana-keygen new --no-outfile
```

> If the `--no-outfile` flag is **omitted**, the default behavior is to write the keypair to `~/.config/solana/id.json`, resulting in a [file system wallet](file-system-wallet.md)

このコマンドの出力には、次のような行が表示されます。

```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

`pubkey:`の後に表示されているのは、あなたの*ウォレットアドレス*です。

**注：**ペーパーウォレットやファイルシステムウォレットでは、"Pubkey"と"ウォレットアドレス"という言葉が混在して使用されることがあります。

> セキュリティを強化するには、 `--word-count` 引数を使用してシードフレーズの単語数を増やします。

詳細は以下を実行してください:

```bash
solana-keygen new --help
```

### 公開キーの派生方法

公開キーはシードフレーズとパスフレーズから派生することができます。 使用する場合はパスフレーズを使用します。 これは、オフラインで生成されたシードフレーズを使って、有効な公開キーを導出する場合に便利です。 `solana-keygen pubkey` コマンドは、シードフレーズとパスフレーズを入力して を実行します。

```bash
solana-keygen pubkey ASK
```

> 同じシードフレーズに異なるパスフレーズを使用する可能性があることに注意してください。 固有のパスフレーズごとに、異なるキーペアが生成されます。

`solana-keygen` ツールはシードフレーズを生成するために同じ BIP39 標準の英単語リストを使用します。 異なる単語リストを使用する他のツールでシードフレーズを生成した場合、`solana-keygen`を使用することはできますが、`--skip-seed-phrase-validation`引数を渡して、この検証を省略する必要があります。

```bash
solana-keygen pubkey ASK --skip-seed-phrase-validation
```

`solana-keygen pubkey ASK` でシードフレーズを入力すると、コンソール は "base-58" 文字列を表示します。 これはシードフレーズに関連付けられている _ウォレットアドレス_ です。

> 派生したアドレスを USB メモリにコピーして、ネットワーク上のコンピュータで簡単に利用できるようにします。

> 次のステップとして、公開キーに関連付けられたアカウントの[残高を確認する](#checking-account-balance)のが一般的です。

詳細は以下を実行してください:

```bash
solana-keygen pubkey --help
```

## キーペアを確認

ペーパーウォレットのアドレスの秘密キーを管理していることを確認するには、`solana-keygen verify`を使用します。

```bash
solana-keygen verify <PUBKEY> ASK
```

`<PUBKEY>` がウォレットアドレスに置き換えられ、 `ASK` がキーペアのシードフレーズを要求するように コマンドに指示します。 セキュリティ上の理由によりシードフレーズは入力中に表示されません。 シードフレーズを入力した後、与えられた公開キーがシードフレーズから生成されたキーペアと一致すれば"Success"、そうでなければ"Failed"と出力されます。

## アカウント残高の確認

アカウントの残高を確認するのに必要なのは、アカウントの公開キーです。 ペーパーウォレットから公開キーを安全に取り出すには、[エアギャップのあるコンピューター](<https://en.wikipedia.org/wiki/Air_gap_(networking)>)で["公開キーの導出"](#public-key-derivation)の手順に従ってください。 その後、公開キーを手動で入力するか、USB スティックを使ってネットワーク接続されたマシンに転送します。

次に、 `solana` CLI ツールを [特定のクラスターに接続するように設定します。](../cli/choose-a-cluster.md):

```bash
solana config set --url <CLUSTER URL> # (i.e. https://api.mainnet-beta.solana.com)
```

最後に、バランスを確認するには、次のコマンドを実行します。

```bash
solana balance <PUBKEY>
```

## 複数のペーパーウォレットアドレスの作成

ウォレットアドレスはいくつでも作成することができます。 Seed Phrase Generation [または](#seed-phrase-generation) [Public Key Derivation](#public-key-derivation) でステップを再実行することで、新しいアドレスを作成できます。 複数のウォレットアドレスは、異なる目的のために自分のアカウント間でトークンを転送したい場合に便利です。

## サポート

[ウォレットサポートページ](support.md)を参考にしてください。
