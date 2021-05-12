---
title: クラスターソフトウェアのインストールと更新
---

現在、ユーザーはgitリポジトリから自分でsolanaクラスタソフトウェアを構築し、手動でアップデートする必要がありますが、これはエラーが発生しやすく不便です。

このドキュメントでは、サポートされているプラットフォーム用に事前に構築されたバイナリを展開するために使用できる、使いやすいソフトウェアのインストールおよびアップデータを提案します。 ユーザーは、Solanaまたは信頼できる他のパーティが提供するバイナリを使用することを選択できます アップデートの展開は、オンチェーンアップデートマニフェストプログラムを使って管理されます。

## モチベーションの例

### ブートストラップのcurl/shellスクリプトを使って、事前に作成されたインストーラを取得して実行します。

サポートされているプラットフォームでの最も簡単なインストール方法です。

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh
```

このスクリプトは、githubで最新のタグ付きリリースをチェックし、そこから`solana-install-init`バイナリをダウンロードして実行します。

インストール時に追加の引数を指定する必要がある場合は、以下のShell構文を使用します。

```bash
$ init_args=.... # arguments for `solana-install-init ...`
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh -s - ${init_args}
```

### Githubのリリースからビルド済みインストーラを取得して実行します。

よく知られているリリースの URL を使用して、サポートされているプラットフォーム用のビルド済みバイナリを取得できます。

```bash
$ curl -o solana-install-init https://github.com/solana-labs/solana/releases/download/v1.0.0/solana-install-init-x86_64-apple-darwin
$ chmod +x ./solana-install-init
$ ./solana-install-init ---help
```

### ソースからインストーラをビルドして実行

あらかじめビルドされたバイナリが特定のプラットフォームで使用できない場合、ソースからインストーラをビルドすることは常にオプションです:

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana/install
$ cargo run -- -help
```

### 新しいアップデートのクラスターへのデプロイ

\(as created by `ci/publish-tarball.sh`\)で作成されたSolanaリリースtarballが公開URLにアップロードされている場合、以下のコマンドでアップデートをデプロイします。

```bash
$ solana-keygen new -o update-manifest.json # <-- 一度のみ生成され、公開キーはユーザーと共有されます
$ solana-install deploy http://example.com/path/to/solana-release.tar.bz2 update-manifest.json
```

### 自動更新するバリデータノードの実行

```bash
$ solana-install init --pubkey 92DMonmBYXwEMHJ99c9ceRSpAmk9v6i3RdvDdXaVcrfj  # <-- pubkey is obtained from whoever is deploying the updates
$ export PATH=~/.local/share/solana-install/bin:$PATH
$ solana-keygen ...  # <-- runs the latest solana-keygen
$ solana-install run solana-validator ...  # <-- runs a validator, restarting it as necesary when an update is applied
```

## On-chain Update Manifest

アップデートマニフェストは、solanaクラスター上で新しいリリースのtarボールの展開を宣伝するために使用されます。 アップデートマニフェストは、`config` プログラムを使用して保存され、各アップデートマニフェストアカウントは、指定されたターゲットトリプル\ \(eg, `x86_64-apple-darwin`\) に対する論理的なアップデートチャネルを記述します。 アカウントの公開キーは、新しいアップデートをデプロイするエンティティと、そのアップデートを消費するユーザーの間でよく知られています。

アップデートのtarball自体は、オフチェーンの別の場所でホストされており、指定された`download_url`から取得できます。

```text
use solana_sdk::signature::Signature;

/// Information required to download and apply a given update
pub struct UpdateManifest {
    pub timestamp_secs: u64, // When the release was deployed in seconds since UNIX EPOCH
    pub download_url: String, // Download URL to the release tar.bz2
    pub download_sha256: String, // SHA256 digest of the release tar.bz2 file
}

/// Data of an Update Manifest program Account.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest: UpdateManifest,
    pub manifest_signature: Signature,
}
```

`manifest` フィールド自体には、対応する署名 \(`manifest_signature`\) が含まれており、 `solana-install` ツールと Solana cluster RPC API の間の中間攻撃を防ぐことに注意してください。

ロールバック攻撃を防ぐため `solana-install` は、現在インストールされているものよりも古い `timestamp_secs` の更新をインストールすることを拒否します。

## リリースアーカイブの内容

リリースアーカイブは、bzip2で圧縮されたtarファイルで、以下のような内部構造になっています。

- `/version.yml` -シンプルなYAMLファイルで、`"target "`というフィールドを含んでいます。

  ターゲットタプル それ以外のフィールドは無視されます。

- `/bin/` --そのリリースで利用可能なプログラムを含むディレクトリ。

  `solana-install` will symlink this directory to

  `~/.local/share/solana-install/bin` で `PATH` 環境で使用します。

  変数

- `... `-- 任意の追加ファイルとディレクトリを許可します。

## solana-install Tool

`solana-install` ツールは、クラスタソフトウェアのインストールと更新に使用されます。

ユーザーのホームディレクトリにある以下のファイルとディレクトリを管理します。

- `~/.config/solana/install/config.yml` - 現在インストールされているソフトウェアバージョンに関するユーザ設定と情報
- `~/.local/share/solana/install/bin` - 現在のリリースへのシンボリックリンク。 eg, `~/.local/share/solana-update/<update-pubkey>-<manifest_signature>/bin`
- `~/.local/share/solana/install/releases/<download_sha256>/` - releaseの内容

### コマンドラインインターフェース

```text
solana-install 0.16.0
The solana cluster software installer

USAGE:
    solana-install [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --config <PATH>    Configuration file to use [default: .../Library/Preferences/solana/install.yml]

SUBCOMMANDS:
    deploy    deploys a new update
    help      Prints this message or the help of the given subcommand(s)
    info      displays information about the current installation
    init      initializes a new installation
    run       Runs a program while periodically checking and applying software updates
    update    checks for an update, and if available downloads and applies it
```

```text
solana-install-init
initializes a new installation

USAGE:
    solana-install init [OPTIONS]

FLAGS:
    -h, --help    Prints help information

OPTIONS:
    -d, --data_dir <PATH>    Directory to store install data [default: .../Library/Application Support/solana]
    -u, --url <URL>          JSON RPC URL for the solana cluster [default: http://devnet.solana.com]
    -p, --pubkey <PUBKEY>    Public key of the update manifest [default: 9XX329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp]
```

```text
solana-install info
displays information about the current installation

USAGE:
    solana-install info [FLAGS]

FLAGS:
    -h, --help     Prints help information
    -l, --local    only display local information, don't check the cluster for new updates
```

```text
solana-install deploy
deploys a new update

USAGE:
    solana-install deploy <download_url> <update_manifest_keypair>

FLAGS:
    -h, --help    Prints help information

ARGS:
    <download_url>               URL to the solana release archive
    <update_manifest_keypair>    Keypair file for the update manifest (/path/to/keypair.json)
```

```text
solana-install update
checks for an update, and if available downloads and applies it

USAGE:
    solana-install update

FLAGS:
    -h, --help    Prints help information
```

```text
solana-install run
Runs a program while periodically checking and applying software updates

USAGE:
    solana-install run <program_name> [program_arguments]...

FLAGS:
    -h, --help    Prints help information

ARGS:
    <program_name>            program to run
    <program_arguments>...    arguments to supply to the program

The program will be restarted upon a successful software update
```
