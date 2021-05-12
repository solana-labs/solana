---
title: "Rustを使った開発"
---

Solana は [Rust](https://www.rust-lang.org/) プログラミング言語を使用してチェーン上のプログラムを書くことをサポートしています。

## プロジェクトのレイアウト

Solana Rust プログラムは典型的な [Rust プロジェクト レイアウト](https://doc.rust-lang.org/cargo/guide/project-layout.html) に従います:

```
/inc/
/src/
/Cargo.toml
```

しかし、次も含める必要があります:

```
/Xargo.toml
```

以下を含める必要があります:

```
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

Solana Rust programs may depend directly on each other in order to gain access to instruction helpers when making [cross-program invocations](developing/programming-model/calling-between-programs.md#cross-program-invocations). このような場合、依存しているプログラムのエントリポイントシンボルを引き込まないようにすることが重要です。 To avoid this, programs should define an `exclude_entrypoint` feature in `Cargo.toml` and use to exclude the entrypoint.

- [Define the feature](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/Cargo.toml#L12)
- [Exclude the entrypoint](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/src/lib.rs#L12)

そして、他のプログラムがこのプログラムを依存関係に含める場合は、`exclude_entrypoint`機能を使って行う必要があります。

- [Include without entrypoint](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token-swap/program/Cargo.toml#L19)

## プロジェクトの依存関係

最低でも Solana Rust のプログラムは[solana-program](https://crates.io/crates/solana-program) crate をプルインしなければなりません。

Solana BPF プログラムにはいくつかの[制限](#restrictions)があり、いくつかのクレートを依存関係として含めることができなかったり、特別な処理が必要だったりします。

例:

- 公式ツールチェーンでサポートされているアーキテクチャのサブセットを必要とするクレート。 これは、そのクレートがフォークされ、BPF がそれらのアーキテクチャをチェックするために追加されない限り、回避することはできません。
- クレートは、Solana の決定論的プログラム環境ではサポートされていない`rand`に依存することがあります `rand`に依存するクレートを含めるには、 [Depending on Rand](#depending-on-rand)を参照してください。
- スタックオーバーフローするコードがプログラム自体に含まれていなくても、クレートがスタックをオーバーフローすることがあります。 詳細については、 [Stack](overview.md#stack) を参照してください。

## ビルド方法

環境をセットアップしてください:

- "https://rustup.rs/" から最新の Rust 安定版をインストールします。
- Install the latest Solana command-line tools from https://docs.solana.com/cli/install-solana-cli-tools

通常のカーゴビルドは、ホストマシンに対してプログラムをビルドし、ユニットテストに使用することができます。

```bash
$ cargo build
```

クラスタにデプロイ可能な Solana BPF ターゲット用に SPL Token などの特定のプログラムを構築します。

```bash
$ cd <the program directory>
$ cargo build-bpf
```

## テスト方法

Solana のプログラムは、プログラムの機能を直接実行することで、従来の`カーゴテスト`の仕組みでユニットテストを行うことができます。

ライブクラスタに近い環境でのテストを容易にするために、開発者は[`プログラムテスト`](https://crates.io/crates/solana-program-test)<クレートを使用することができます。 `program-test` crate は、ランタイムのローカルなインスタンスを起動し、テストの期間中、状態を維持しながら複数のトランザクションを送信することができます。

詳細については、 [sysvar の例](https://github.com/solana-labs/solana-program-library/blob/master/examples/rust/sysvar/tests/functional.rs)のテストは、syavar アカウントを含む命令がどのように送信され、プログラムによって処理されるかを示しています。

## プログラムのエントリポイント

プログラムは既知のエントリポイントシンボルをエクスポートし、Solana ランタイムはプログラムを呼び出す際にこれを検索して呼び出します。 Solana は複数の[バージョンの BPF ローダー](overview.md#versions)をサポートしており、エントリポイントはそれらの間で異なる場合があります。 プログラムは同じローダー用に作成し、同じローダーにデプロイする必要があります。 詳細は [の概要](overview#loaders) をご覧ください。

現在サポートされているローダーは[BPF ローダー](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)と[BPF ローダー deprecated](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)の 2 つです。

どちらも生のエントリーポイントの定義は同じで、以下のような生のシンボルをランタイムが調べて呼び出します。

```rust
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64;
```

このエントリポイントは、シリアル化されたプログラム パラメータ(プログラム Id、アカウント、命令データなど) を含む汎用バイト配列を取ります。 パラメータをデシリアライズするために、各ローダーには独自のラッパーマクロが含まれており、生のエントリポイントをエクスポートし、パラメータをデシリアライズし、ユーザー定義の命令処理関数を呼び出し、結果を返します。

エントリポイントマクロはこちらからご覧いただけます。

- [BPF Loader's entrypoint macro](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)
- [BPF Loader deprecated's entrypoint macro](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

エントリポイントマクロが呼び出すプログラム定義の命令処理関数は、この形式でなければなりません。

```rust
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;
```

[ エントリポイント](https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15) の helloworld による使用を参照してください。

### パラメータデシリアライゼーション

各ローダーには、プログラムの入力パラメータを Rust 型にデシリアライズするヘルパー関数が用意されています。 エントリポイントマクロは、自動的にデシリアライズヘルパーを呼び出します。

- [BPF Loader deserialization](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)
- [BPF Loader deprecated deserialization](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

プログラムの中には、自分でデシリアライズを行いたいものもあるでしょう。その場合は、[生のエントリポイント](#program-entrypoint)の独自の実装を提供します。 提供されているデシリアライズ関数は、プログラムが変更を許可されている変数(lamports、アカウントデータ) については、シリアライズされたバイト配列への参照を保持していることに注意してください。 この理由は、リターン時にローダーがこれらの変更を読み取り、コミットするためです。 プログラムが独自のデシリアライズ関数を実装する場合、プログラムがコミットしたいと思う変更が入力バイト配列に書き戻されるようにする必要があります。

ローダーがプログラムの入力をどのようにシリアライズするかの詳細は、[Input Parameter Serialization](overview.md#input-parameter-serialization)のドキュメントに記載されています。

### データタイプ

ローダーのエントリポイントマクロは、プログラムで定義された命令プロセッサ関数を以下のパラメータで呼び出します。

```rust
program_id: &Pubkey,
accounts: &[AccountInfo],
instruction_data: &[u8]
```

"program_id" は、現在実行中のプログラムの公開キーです。

Account は、命令で参照されるアカウントを順番に並べたもので、[AccountInfo](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/account_info.rs#L10)構造体として表現されます。 例えば、ランポートを転送する場合、1 つ目のアカウントを転送元、2 つ目のアカウントを転送先として指定することができます。

`AccountInfo`構造体のメンバーは、`lamports`と`data`以外は読み取り専用です どちらも、[ランタイムの実施ポリシー](developing/programming-model/accounts.md#policy)に従って、プログラムが変更することができます。 これらのメンバーはどちらも Rust の`RefCell`構文で保護されているため、読み書きするには借用しなければなりません。 その理由は、どちらも元の入力バイト配列を指していますが、accounts slice には同じアカウントを指している複数のエントリが存在する可能性があるからです。 `RefCell`を使用することで、プログラムが複数の`AccountInfo`構造体を介して同じ基礎データに対する重複した読み取り/書き込みを誤って実行しないようにします。 プログラムが独自のデシリアライズ関数を実装する場合は、重複したアカウントを適切に処理するように注意する必要があります。

[命令データ](developing/programming-model/transactions.md#instruction-data)は、処理される命令の命令データからの汎用バイト配列です。

## ヒープ

Rust プログラムはカスタム [`global_allocator`](https://github.com/solana-labs/solana/blob/8330123861a719cd7a79af0544617896e7f00ce3/sdk/program/src/entrypoint.rs#L50) を定義して直接ヒープを実装します。

プログラムは固有の必要性に基づいて独自の `global_allocator` を実装することができます。 詳細については、 [カスタムヒープの例](#examples) を参照してください。

## 制限事項

オンチェーンの Rust プログラムは、Rust の"libstd"、"libcore"、"liballoc"のほとんどと、多くのサードパーティークレートをサポートしています。

これらのプログラムは、リソースに制約のあるシングルスレッド環境で実行されるため、いくつかの制限があり、決定論的でなければなりません。

- アクセス権限がありません：
  - `rand`
  - `std::fs`
  - `std::net`
  - `std::os`
  - `std::future`
  - `std::net`
  - `std::process`
  - `std::sync`
  - `std::task`
  - `std::thread`
  - `std::time`
- アクセス制限:
  - `std::hash`
  - `std::os`
- Bincode は、サイクル数とコールデプスの両方で非常に計算量が多いので、避けるべきです。
- 文字列のフォーマットも計算量が多いため、避けるべきです。
- `println!`, `print!` をサポートしていないので、Solana の[ログヘルプ](#logging)を使用してください。
- ランタイムは、1 つの命令の処理中にプログラムが実行できる命令の数に制限を設けています See [computation budget](developing/programming-model/runtime.md#compute-budget) for more information.

## Rand によって異なります

プログラムは決定論的に実行するように制約されているため、乱数は利用できません。 プログラムが乱数機能を使用していなくても、プログラムが`rand`に依存するクレートに依存している場合があります。 プログラムが `rand`に依存している場合、Solana の `get-random` サポートがないため、コンパイルは失敗します。 エラーは通常、次のようになります。

```
error: target is not supported, for more information see: https://docs.rs/getrandom/#unsupported-targets
   --> /Users/jack/.cargo/registry/src/github.com-1ecc6299db9ec823/getrandom-0.1.14/src/lib.rs:257:9
    |
257 | /         compile_error!("\
258 | |             target is not supported, for more information see: \
259 | |             https://docs.rs/getrandom/#unsupported-targets\
260 | |         ");
    | |___________^
```

この依存関係の問題を回避するには、プログラムの`Cargo.toml`に以下の依存関係を追加します。

```
getrandom = { version = "0.1.14", features = ["dummy"] }
```

## ログ

Rust の `println!` マクロは計算量が高く、サポートされていません。 代わりに ヘルパーマクロ [`msg!`](https://github.com/solana-labs/solana/blob/6705b5a98c076ac08f3991bb8a6f9fcb280bf51e/sdk/program/src/log.rs#L33) が提供されています。

`msg!` には 2 つの形式があります:

```rust
msg!("A string");
```

または

```rust
msg!(0_64, 1_64, 2_64, 3_64);
```

両方のフォームは、結果をプログラムログに出力します。 プログラムが望めば、`format!`を使って`println!`をエミュレートすることができます。

```rust
msg!("Some variable: {:?}", variable);
```

[デバッグ](debugging.md#logging)のセクションでは、プログラムログの扱い方について詳しく説明しています。[Rust の例](#examples)では、ログ取得の例を紹介しています。

## パニック状態

Rust の`panic!`, `assert!`, internal panic の結果は、デフォルトで[プログラムログ](debugging.md#logging)に出力されます。

```
INFO  solana_runtime::message_processor] Finalized account CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Call BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Program log: Panicked at: 'assertion failed: `(left == right)`
      left: `1`,
     right: `2`', rust/panic/src/lib.rs:22:5
INFO  solana_runtime::message_processor] BPF program consumed 5453 of 200000 units
INFO  solana_runtime::message_processor] BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ failed: BPF program panicked
```

### カスタムパニックハンドラー

プログラムは、独自の実装を提供することで、デフォルトのパニックハンドラをオーバーライドすることができます。

最初にプログラムの `Cargo.toml` の `custom-panic` 機能を定義します。

```toml
[features]
default = ["custom-panic"]
custom-panic = []
```

次に、パニックハンドラのカスタム実装を提供します。

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    solana_program::msg!("program custom panic enabled");
    solana_program::msg!("{}", info);
}
```

上のスニピットでは、デフォルトの実装が示されていますが、開発者はこれをよりニーズに合ったものに置き換えることができます。

パニックメッセージをデフォルトでサポートすることの副作用として、プログラムの共有オブジェクトに Rust の`libstd`実装をより多く取り込む必要があります。 一般的なプログラムでは、すでにかなりの量の libstd を取り込んパニックメッセージをデフォルトでサポートすることの副作用として、プログラムの共有オブジェクトに Rust の`libstd`実装をより多く取り込むというコストが発生します。 しかし、 `libstd` を避けることによって、明示的に非常に小さくしようとするプログラムは、大きな影響を与える可能性があります(〜25kb)。 この影響をなくすために、プログラムは空の実装で独自のカスタム・パニック・ハンドラを提供することができます。

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Do nothing to save space
}
```

## 予算の計算

システムコール[`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/program/src/log.rs#L102) を使用して、プログラムが実行を停止するまでに消費できるコンピュートユニットの残りの数を示すメッセージを記録します。

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF ダンプ

BPF 共有オブジェクトの内部をテキストファイルにダンプすることで、プログラムの構成や実行時に何をしているかをより詳しく知ることができます。 ダンプには、ELF 情報のほか、すべてのシンボルとそれを実装する命令のリストが含まれます。 BPF ローダーのエラーログメッセージの中には、エラーが発生した特定の命令番号を参照しているものがあります。 これらの参照は、問題のある命令とそのコンテキストを特定するために ELF ダンプで検索することができます。

ダンプファイルを作成するには:

```bash
$ cd <program directory>
$ cargo build-bpf
```

## 例:

[Solana Program Library github](https://github.com/solana-labs/solana-program-library/tree/master/examples/rust) リポジトリには、Rust 例のコレクションが含まれています。
