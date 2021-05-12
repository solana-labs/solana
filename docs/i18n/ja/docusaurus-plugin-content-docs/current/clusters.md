---
title: ソラナクラスタ
---

Solanaはさまざまな目的でいくつかの異なるクラスタを維持しています。

始める前、最初に[Solana コマンドラインツールをインストールしたこと](cli/install-solana-cli-tools.md)を確認してください。

エクスプローラ：

- [http://explorer.solana.com/](https://explorer.solana.com/).
- [http://solanabeach.io/](http://solanabeach.io/).

## Devnet

- "Devnet" は、Solana をテストドライブとして利用したい人のための遊び場として機能します。 ユーザー、トークンホルダー、アプリ開発者、またはバリデータとして活用してください。
- アプリケーション開発者は"Devnet"をターゲットにするといいでしょう。
- 潜在的な検証者は、最初に"Devnet" をターゲットにするといいでしょう。
- Devnet と Mainnet ベータの主な違い:
  - Devnet トークンは **本物ではありません**。
  - Devnet には、アプリケーションテスト用のエアドロップ用のトークン蛇口が含まれています。
  - Devnet には台帳リセットの対象となる可能性があります
  - Devnet は通常、Mainnet Betaよりも新しいソフトウェアバージョンを実行します。
- Devnet のゴシップエントリポイント: `entrypoint.devnet.solana.com:8001`
- Devnet のメトリック環境変数:
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=devnet,u=scratch_writer,p=topsecret"
```
- RPC URL for Devnet: `https://devnet.solana.com`

##### コマンドライン構成の例 `solana`

```bash
solana config set --url https://devnet.solana.com
```

##### Example `solana-validator` command-line

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator dv1LfzJvDF7S1fBKpFgKoKXK5yoSosmkAdfbxBo1GqJ \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.devnet.solana.com:8001 \
    --expected-genesis-hash EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

The `--trusted-validator`s is operated by Solana

## テストネット

- "Testnet"は、ライブクラスタ、特にネットワークパフォーマンス、安定性、バリデータの動作に焦点を当てた最近のリリース機能をテストする場所です。
- ["Tour de SOL"](tour-de-sol.md)は"Testnet"上で行われ、ネットワーク上での悪意のある行動や攻撃を推奨しています。 テストネットでは、ネットワーク上での悪意のある行動や攻撃を奨励し、バグやネットワークの脆弱性を発見し、潰すことに役立てています。 ネットワーク上で悪意のある行動や攻撃を行うことで、バグやネットワークの脆弱性を発見し、潰すことができます。
- テストネットトークンは **本物ではありません**。
- Testnet には台帳リセットの対象となる可能性があります。
- Testnet には、アプリケーションテスト用のエアドロップ用のトークン蛇口が含まれています
- Testnet では通常、Devnet と Mainnet Beta よりも新しいソフトウェアリリースを実行します。
- テストネットのゴシップエントリーポイント: `entrypoint.testnet.solana.com:8001`
- Testnet のメトリック環境変数:
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=testnet_write,p=c4fa841aa918bf8274e3e2a44d77568d9861b3ea"
```
- テストネットの RPC URL: `https://testnet.solana.com`

##### コマンドライン構成の例 `solana`

```bash
solana config set --url https://testnet.solana.com
```

##### Example `solana-validator` command-line

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --trusted-validator ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T \
    --trusted-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --trusted-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

`--trusted-validator`s のIDは以下のとおりです。

- `5D1fNXzv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` - testnet.solana.com (Solana)
- `ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T` - RPCノードを破壊 (ソラナ)
- `Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - Certus One
- `9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - Algo|Stake

## メインネットベータ

初期のトークンホルダとローンチパートナーのための、パーミッションレスで永続的なクラスタです。 現在、報酬とインフレは無効になっています。

- "Mainnet Beta"で発行されたトークンは **本物** のSOLです。
- CoinList競売などのトークンを購入/発行するためにお金を支払った場合は、これらのトークンはMainnet Betaで転送されます。
  - 注意: [Solflare](wallet-guide/solflare.md)のようなコマンドライン以外のウォレットを使用している場合、 ウォレットは常にメインネットベータに接続されます。
- テストネットのゴシップエントリポイント: `entrypoint.mainnet-beta.solana.com:8001`
- Mainnet Beta のメトリック環境変数:
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=mainnet-beta,u=mainnet-beta_write,p=password"
```
- Mainnet Beta用RPC URL: `https://api.mainnet-beta.solana.com`

##### コマンドラインの構成例 `solana`

```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### Example `solana-validator` command-line

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --private-rpc \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint2.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint3.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint4.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint5.mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

4 つのすべて `--trusted-validator`s は Solana によって運営されています
