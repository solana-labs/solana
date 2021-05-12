---
title: クラスタのベンチマーク
---

Solana の"git"リポジトリには、あなた自身のローカルテストネットを立ち上げるために必要なスクリプトがすべて含まれています。 本格的でパフォーマンスの高いマルチノードのテストネットは、Rust のみのシングルノードのテストノードよりも設定がかなり複雑なので、実現したいことによっては別のバリエーションを実行したほうがいいかもしれません。 スマートコントラクトの実験など、ハイレベルな機能の開発を検討している場合は、セットアップの手間を省くために、Rust のみのシングルノードでのデモを利用してください。 トランザクションパイプラインのパフォーマンス最適化を行う場合は、"enhanced singlenode"デモを検討してください。 コンセンサスを行う場合は、最低でも Rust のみのマルチノードデモが必要です。 TPS 測定値を再現したい場合は、"enhanced multinode"デモを実行してください。

この 4 つのバリエーションには、最新の Rust ツールチェーンと Solana のソースコードが必要です。

まず、Rust、Cargo、システムパッケージを Solana の[README](https://github.com/solana-labs/solana#1-install-rustc-cargo-and-rustfmt)に記載されている通りにセットアップします。

それでは、github からコードをチェックアウトしてみましょう。

```bash
git clone https://github.com/solana-labs/solana.git
cd solana
```

デモコードは、低レベルの新機能を追加するため、リリースの間に壊れてしまうことがあります。そのため、初めてデモを実行する場合は、[最新のリリース](https://github.com/solana-labs/solana/releases)を確認してから進めていただくと成功率が高まります。

```bash
TAG=$(git description --tags $(git rev-list --tags --max-count=1))
git checkout $TAG
```

### 構成設定

ノードが起動する前に、投票プログラムなどの重要なプログラムが確実にビルドされていることを確認します。 ここでは、パフォーマンスを上げるためにリリースビルドを使用していることに注意してください。 デバッグビルドが必要な場合は、`cargo build`を使用し、コマンドの`NDEBUG=1`の部分を省略してください。

```bash
cargo build --release
```

ネットワークは、以下のスクリプトを実行して生成されたジェネシスレジャーで初期化されます。

```bash
NDEBUG=1 ./multinode-demo/setup.sh
```

### 蛇口

バリデータとクライアントを動作させるためには、テストトークンを配るための蛇口を回す必要があります。 蛇口は、テストトランザクションで使用されるミルトンフリードマンスタイルの「エアドロップ」\(顧客に要求するフリートークン\) を提供します。

蛇口を起動するには以下のようにしてください。

```bash
NDEBUG=1 ./multinode-demo/faucet.sh
```

### シングルノードテストネット

バリデータを起動する前に、デモ用のブートストラップバリデータにしたいマシンの IP アドレスを確認し、テストしたいすべてのマシンで、udp ポート 8000-10000 が開いていることを確認してください。

それでは、別のシェルでブートストラップバリデータを起動してください。

```bash
NDEBUG=1 ./multinode-demo/bootstrap-validator.sh
```

サーバーが初期化されるまで数秒待ちます。 トランザクションを受信できる状態になると、"leader ready..."と表示されます。 リーダーは、トークンを持っていない場合、蛇口からトークンを要求します。 その後のリーダーの起動では、蛇口が動いている必要はありません。

### マルチノードテストネット

マルチノードのテストネットを実行するには、リーダーノードを起動した後、別のシェルでいくつかのバリデータをスピンアップします。

```bash
NDEBUG=1 ./multinode-demo/validator-x.sh
```

パフォーマンスが向上したバリデータを Linux で実行するには、[CUDA 10.0](https://developer.nvidia.com/cuda-downloads)がシステムにインストールされている必要があります。

```bash
./fetch-perf-libs.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/bootstrap-validator.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/validator.sh
```

### テストネットクライアントのデモ

シングルノードまたはマルチノードのテストネットが稼働したので、トランザクションを送信してみましょう。

別のシェルで、クライアントを起動します。

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh # runs against localhost by default
```

何が起こったのでしょう？ クライアントのデモでは、いくつかのスレッドを立ち上げて、50 万件のトランザクションをテストネットにできるだけ早く送信します。 その後、クライアントは定期的にテストネットに ping を実行し、その間に処理したトランザクションの数を確認します。 このデモでは、ネットワークがほぼ確実に大量の UDP パケットをドロップするように、意図的にネットワークをフラッディングしていることに注意してください。 これにより、テストネットが 710k TPS に到達する機会が確保されます。 クライアントのデモは、テストネットがそれ以上のトランザクションを処理しないことを確信した後に終了します。 いくつかの TPS 測定値が画面に表示されるはずです。 マルチノードの場合には、各バリデータノードの TPS 測定値も表示されます。

### テストネットのデバッグ

コードには便利なデバッグメッセージがあり、モジュールごと、レベルごとに有効にすることができます。 リーダーやバリデータを実行する前に、通常の"RUST_LOG"環境変数を設定してください。

例

- "solana::banking_stage"モジュールで、どこでも`info`を有効にし、`デバッグ`のみを有効にします。

  ```bash
  export RUST_LOG=solana=info,solana::banking_stage=debug
  ```

- BPF プログラムのログを有効にするには以下のようにします。

  ```bash
  export RUST_LOG=solana_bpf_loader=trace
  ```

一般的には、頻度の低いデバッグメッセージには`debug`、頻度の高いメッセージには`trace`、パフォーマンス関連のログには`info`を使用します。

GDB を使って実行中のプロセスにアタッチすることもできます。 リーダーのプロセスの名前は*solana-validator*です。

```bash
sudo gdb
attach <PID>
set logging on
thread apply all bt
```

これは、すべてのスレッドのスタックトレースを"gdb.txt"にダンプします。

## 開発者用テストネット

この例では、クライアントは私たちの公開テストネットに接続します。 テストネット上でバリデータを実行するには、 udp ポート`8000-10000`を開く必要があります。

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh --entrypoint devnet.solana.com:8001 --faucet devnet.solana.com:9900 --duration 60 --tx_count 50
```

取引の効果は、[メトリクスダッシュボード](https://metrics.solana.com:3000/d/monitor/cluster-telemetry?var-testnet=devnet)で確認できます。
