---
title: "C を使った開発"
---

Solana は、C や C++といったプログラミング言語を使ったオンチェーンプログラムの作成をサポートしています。

## プロジェクトのレイアウト

C のプロジェクトは以下のようにレイアウトされています。

```
/src/<program name>
/makefile
```

`makefile` には以下を含める必要があります。

```bash
OUT_DIR := <path to place to resulting shared object>
include ~/.local/share/solana/install/active_release/bin/sdk/bpf/c/bpf.mk
```

Bpf-sdk は上記で指定した場所にはないかもしれませんが、[How to Build](#how-to-build)に従って環境を整えれば、そのようになるはずです。

C プログラムの例として、[helloworld](https://github.com/solana-labs/example-helloworld/tree/master/src/program-c)を見てみましょう。

## ビルド方法

環境をセットアップしてください:

- "https://rustup.rs"から最新の Rust 安定版をインストールします
- "https://docs.solana.com/cli/install-solana-cli-tools"から最新の Solana コマンドラインツールをインストールします。

次に、make を使用してビルドします:

```bash
make -C <program directory>
```

## テスト方法

Solana では、[Criterion](https://github.com/Snaipe/Criterion)のテストフレームワークを使用しており、プログラムがビルドされるたびにテストが実行されます[How to Build](#how-to-build)]。

テストを追加するには、ソースファイルの隣に`test_<program name>.c` という名前の新しいファイルを作成し、そこに Criterion のテストケースを入力します。 テストケースの書き方については、[helloworld C](https://github.com/solana-labs/example-helloworld/blob/master/src/program-c/src/helloworld/test_helloworld.c) テストや [Criterion](https://criterion.readthedocs.io/en/master) のドキュメントを参照してください。

## プログラムのエントリポイント

プログラムは既知のエントリポイントシンボルをエクスポートし、Solana ランタイムはプログラムを呼び出す際にこれを検索して呼び出します Solana は複数の[バージョンの BPF ローダー](overview.md#versions)をサポートしており、エントリポイントはそれらの間で異なる場合があります。 プログラムは同じローダー用に作成し、同じローダーにデプロイする必要があります。 詳細は [の概要](overview#loaders) をご覧ください。

プログラムはランタイムローダーによってデプロイされ、実行されます。 現在、サポートされている 2 つのローダー [BPF ローダー](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)と [BPF ローダー 非推奨](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)です。

どちらも生のエントリーポイントの定義は同じで、以下のようにランタイムが調べて呼び出す生のシンボルがあります。

```c
extern uint64_t entrypoint(const uint8_t *input)
```

このエントリポイントは、シリアル化されたプログラム パラメータ(プログラム Id、アカウント、命令データなど) を含む汎用バイト配列を取ります。 パラメータをデシリアライズするために、各ローダーには[独自のヘルパー関数](#Serialization)が含まれています。

[ エントリポイント](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L37) の helloworld による使用を参照してください。

### シリアライゼーション

[deerialization 関数](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43) の helloworld による使用を参照してください。

各ローダーには、プログラムの入力パラメータを C 言語の型にデシリアライズするヘルパー関数が用意されています。

- [BPF Loader deserialization](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L304)
- [BPF Loader deprecated deserialization](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

プログラムによっては、自分でシリアル化解除を行いたい場合がありますが、その場合は、[生のエントリポイント](#program-entrypoint)の独自の実装を提供します。 プログラムが変更できる変数(ランポート、アカウントデータ) については、提供されたデシリアライズ関数がシリアライズされたバイト配列への参照を保持することに注意してください。 この理由は、リターン時にローダーがこれらの変更を読み取り、コミットするためです。 プログラムが独自のデシリアライズ関数を実装する場合は、プログラムがコミットしたいと思う変更が入力バイト配列に書き戻されなければならないことを確認する必要があります。

ローダーがプログラムの入力をどのようにシリアライズするかの詳細は、["Input Parameter Serialization"](overview.md#input-parameter-serialization)のドキュメントに記載されています。

## データタイプ

ローダーのデシリアライズヘルパー関数が[SolParameters](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276)構造体を生成します。

```c
/**
 * Structure that the program's entrypoint input data is deserialized into.
 */
typedef struct {
  SolAccountInfo* ka; /** Pointer to an array of SolAccountInfo, must already
                          point to an array of SolAccountInfos */
  uint64_t ka_num; /** Number of SolAccountInfo entries in `ka` */
  const uint8_t *data; /** pointer to the instruction data */
  uint64_t data_len; /** Length in bytes of the instruction data */
  const SolPubkey *program_id; /** program_id of the currently executing program */
} SolParameters;
```

"ka"は、命令で参照されるアカウントの順序付き配列で、[SolAccountInfo](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L173)構造体として表されます。 例えば、ランポートを転送する際に、最初のアカウントを転送元、2 番目のアカウントを転送先とする命令がありますが、配列内のアカウントの位置はその意味を表します。

`SolAccountInfo`構造体のメンバは、`lamports`と`data`以外は読み取り専用です。 どちらも[ランタイムエンフォースメントポリシー](developing/programming-model/accounts.md#policy)に従って、プログラムによって変更される可能性があります。 1 つの命令が同じアカウントを複数回参照する場合、配列内の`SolAccountInfo`エントリが重複することがありますが、それらはいずれも元の入力バイト配列を指しています。 プログラムはこのようなケースを慎重に扱い、同じバッファへの読み書きが重ならないようにしなければなりません。 プログラムが独自のデシリアライズ関数を実装する場合は、重複するアカウントを適切に処理するように注意する必要があります。

`data`は、[処理中の命令の命令データ](developing/programming-model/transactions.md#instruction-data)からの汎用バイト配列です。

`program_id` は、現在実行中のプログラムの公開キーです。

## ヒープ

C プログラムは、システムコール[`calloc`](https://github.com/solana-labs/solana/blob/c3d2d2134c93001566e1e56f691582f379b5ae55/sdk/bpf/c/inc/solana_sdk.h#L245)でメモリを割り当てたり、仮想アドレス x300000000 から始まる 32KB のヒープ領域の上に独自のヒープを実装することができます。 ヒープ領域は`calloc`でも使用されるため、プログラムが独自のヒープを実装する場合は、`calloc`も呼び出すべきではありません。

## ログ

ランタイムは、データを取得し、プログラム ログに記録する 2 つのシステムコールを提供します。

- [`sol_log(const char*)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L128)
- [`sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L134)

[デバッグ](debugging.md#logging) セクションには、プログラムログを使用する に関する詳細情報があります。

## 予算の計算

システムコール[`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/bpf/c/inc/solana_sdk.h#L140) を使用して、プログラムが実行を停止するまでに消費できるコンピュートユニットの残りの数を示すメッセージを記録します。

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF ダンプ

BPF 共有オブジェクトの内部をテキストファイルにダンプすることで、プログラムの構成や実行時に何をしているかをより詳しく知ることができます。 ダンプには、ELF 情報のほか、すべてのシンボルとそれを実装する命令のリストが含まれます。 BPF ローダーのエラーログメッセージの中には、エラーが発生した特定の命令番号を参照しているものがあります。 これらの参照は、問題のある命令とそのコンテキストを特定するために ELF ダンプで検索することができます。

ダンプファイルを作成するには:

```bash
$ cd <program directory>
$ make dump_<program name>
```

## 例:

[Solana Program Library github](https://github.com/solana-labs/solana-program-library/tree/master/examples/c) repo には、C 言語のサンプル集が含まれています。
