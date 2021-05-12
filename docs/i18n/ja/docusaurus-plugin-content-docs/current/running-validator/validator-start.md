---
title: バリデータの開始
---

## Solana CLI の設定

"solana cli" には `get` と `設定コマンドを自動的に` に設定します。 cli コマンドの `--url` 引数を設定します。 例:

```bash
solana config set --url http://devnet.solana.com
```

このセクションでは、Devnet クラスターへの接続方法を説明しますが、手順 は、他の [Solana Clusters](../clusters.md) に似ています。

## クラスタが到達可能かどうかの確認

バリデータノードをアタッチする前に、トランザクションカウントを取得してクラスタが自分のマシンからアクセス可能かどうかをサニティチェックします。

```bash
solanaトランザクション数
```

クラスタのアクティビティの詳細については、[メトリクスダッシュボード](https://metrics.solana.com:3000/d/monitor/cluster-telemetry)をご覧ください。

## インストールの確認

以下のコマンドを実行して、ゴシップネットワークに参加し、クラスタ内の他のノードを表示してみてください。

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
# Press ^C to exit
```

## CUDA を有効にしています

CUDAがインストール\(Linux-only currently\) されたGPUが搭載されている場合は、`solana-validator`の引数に`--cuda`を指定してください。

バリデータの起動時に、CUDAが有効であることを示す以下のログメッセージを確認してください。`"[<timestamp> solana::validator] CUDA is enabled"`

## システムチューニング

### Linux
#### 自動設定
Solanaのレポには、システム設定を調整してパフォーマンスを最適化するためのデーモンが含まれています(すなわち、OSのUDPバッファとファイルマッピングの制限を増やすことで)。

このdaemon(`solana-sys-tuner`) は、Solanaのバイナリリリースに含まれています。 最新の推奨設定を適用するには、ソフトウェアのアップグレード後にバリデータを再起動する*前*にdaemonを再起動してください。

実行するには:

```bash
sudo solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
```

#### 手動操作
自分でシステムの設定を管理したい場合は、以下のコマンドで設定できます。

##### **UDP バッファーを増加**
```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-udp-buffers.conf <<EOF
# Increase UDP buffer size
net.core.rmem_default = 134217728
net.core.rmem_max = 134217728
net.core.wmem_default = 134217728
net.core.wmem_max = 134217728
EOF"
```
```bash
sudo sysctl -p /etc/sysctl.d/20-solana-udp-buffers.conf
```

##### **メモリにマップされたファイルの上限を増加**
```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-mmaps.conf <<EOF
# Increase memory mapped files limit
vm.max_map_count = 500000
EOF"
```
```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```
追加
```
LimitNOFILE=500000
```
systemdサービスファイルの `[Service]` セクションに、 それ以外の場合は追加する
```
DefaultLimitNOFILE=500000
```
`etc/systemd/system.conf`の`[Manager]`セクションに追加します。
```bash
sudo systemctl daemon-reload
```
```bash
sudo bash -c "cat >/etc/security/limits.d/90-solana-nofiles.conf <<EOF
# Increase process file descriptor count limit
* - nofile 500000
EOF"
```
```bash
### Close all open sessions (log out then, in again) ###
```

## Generate identity

以下を実行して、バリデータの識別キーペアを作成します。

```bash
solana-keygen new -o ~/validator-keypair.json
```

以下を実行することで、Id 公開キーを表示できるようになりました:

```bash
solana-keygen pubkey ~/validator-keypair.json
```

> 注意: "validator-keypair.json" ファイルは \(ed25519\)秘密キーでもあります。

### ペーパーウォレットID

キーペアファイルをディスクに書き込む代わりに、本人確認用のペーパーウォレットを作成できます。

```bash
solana-keygen new --no-outfile
```

これで、対応するID公開キーを実行して見ることができます。

```bash
solana-keygen pubkey ASK
```

そしてシードフレーズを入力します

詳細は [ペーパーウォレットの使用法](../wallet-guide/paper-wallet.md) を参照してください。

---

### Vanity Keypair

"solana-keygen"を使って、カスタムバニティキーペアを生成することができます。 例えば:

```bash
solana-keygen grind --starts-with e1v1s:1
```

要求された文字列によっては、マッチを見つけるのに数日かかることがあります。

---

バリデータのIDキーペアは、ネットワーク上であなたのバリデータを一意に識別するものです。 **この情報をバックアップしておくことは非常に重要です。**

この情報をバックアップしておかないと、万が一バリデータにアクセスできなくなったときに、それを復元することができなくなります。 そうなると、あなたのSOLの割り当ても失われてしまいます。

バリデータを識別するキーペアをバックアップするには、 ** "validator-keypair.json" ファイルまたはシードフレーズを安全な場所にバックアップします。**

## 追加の Solana CLI 設定

これでキーペアができたので、以下のすべてのコマンドでバリデータのキーペアを使用するようにSolanaの設定を行います。

```bash
solana config set --keypair ~/validator-keypair.json
```

次の出力が表示されます。

```text
ウォレット設定を更新しました: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## エアドロと& バリデータ残高の確認

開始するにはトークンをいくつかドロップしてください:

```bash
10SOLをエアドロップ
```

エアドロップは"Devnet"と"Testnet"でのみ利用可能であることに注意してください。 どちらも1リクエストにつき10SOLまでとなります。

現在の残高を表示するには:

```text
solana balance
```

または詳細を確認するには:

```text
solana balance --lamports
```

SOLとラムポートの [の違いについては、こちら](../introduction.md#what-are-sols)をご覧ください。

## 投票アカウントを作成

まだ行っていない場合は、投票アカウントのキーペアを作成し、ネットワーク上に 投票アカウントを作成してください。 このステップを完了した場合は、Solana ランタイムディレクトリに “vote-account-keypair.json” が表示されます。

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

以下のコマンドは、すべてのデフォルトオプションを使用して、ブロックチェーン に投票アカウントを作成するために使用できます。

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

[投票アカウントの作成と管理](vote-accounts.md) についての詳細をご覧ください。

## 信頼されたバリデータ

他のバリデータノードを知っていて信頼している場合は、コマンドラインで`solana-validator`の`--trusted-validator <PUBKEY>`引数で指定できます。 引数 `--trusted-validator<PUBKEY1> --trusted-validator <PUBKEY2>`を繰り返すことで、複数のバリデータを指定することができます。 これには2つの効果があり、1つはバリデータが` --no-untrusted-rpc` で起動しているときに、ジェネシスデータとスナップショットデータをダウンロードする際に、信頼されているノードのセットにのみ問い合わせを行うことです。 もうひとつは、`--halt-on-trusted-validator-hash-mismatch`オプションとの組み合わせで、gossip上の他の信頼されたノードのアカウント状態全体のmerkleルートハッシュを監視し、ハッシュに不一致があった場合には、バリデータが不正な状態値を投票したり処理したりするのを防ぐために、バリデータはノードを停止するというものです。 現時点では、バリデータがハッシュを公開するスロットはスナップショットの間隔と連動しています。 この機能を有効にするためには、信頼するセット内のすべてのバリデータの スナップショット間隔を同じにするか、その倍数にする必要があります。

悪意のあるスナップショット状態のダウンロードやアカウント状態の乖離を防ぐために、これらのオプションを使用することを強くお勧めします。

## バリデータに接続

クラスタに接続するには、次を実行します。

```bash
solana-validator \
  --identity ~/validator-keypair.json \
  --vote-account ~/vote-account-keypair.json \
  --ledger ~/validator-ledger \
  --rpc-port 8899 \
  --entrypoint devnet.solana.com:8001 \
  --limit-ledger-size \
  --log ~/solana-validator.log
```

バリデータのログを強制的にコンソールに出力するには` --log - `引数を追加します。

> 注:  [ペーパーウォレットのシードフレーズ](./wallet-guide/paper-wallet.md)には、`--identity`および/または`--authorized-voter`のキーペアを指定します。 これらを使用するには、`solana-validator --identity ASK ... --authorized-voter ASK ...`のようにそれぞれの引数を渡すと、シードフレーズとオプションのパスフレーズを入力するように促されます。

バリデータがネットワークに接続されていることを、新しいターミナルを開いて実行して確認します。

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

バリデータが接続されていれば、その公開キーとIPアドレスがリストに表示されます。

### ローカルネットワークポート割り当ての制御

デフォルトでは、バリデータは 8000-10000 の範囲で利用可能なネットワークポートを動的に選択しますが、 `--dynamic-port-range` でオーバーライドすることもできます。 たとえば、`solana-validator --dynamic-port-range 11000-11010 ...` とすると、バリデータの対象をポート "11000-11010" に制限します。

### ディスク領域を節約するための台帳サイズの制限
`--limit-ledger-size`パラメータでは、ノードがディスク上に保持する台帳の数を[shred](./terminology.md#shred)で指定できます。 このパラメータを指定しなかった場合、バリデータはディスクの空き容量がなくなるまで台帳全体を保持します。

デフォルト値では、台帳のディスク使用量を 500GB 未満に抑えようとします。  必要に応じて` --limit-ledger-size` に引数を追加することで、ディスク使用量の増減を要求できます。 `solana-validator --help` で `--limit-ledger-size` で使用されるデフォルトの制限値を確認してください。  カスタムリミット値を選択する方法の詳細は [こちら ](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26) で確認してください。

### システムユニット
バリデータを"systemd"ユニットとして実行することは、バックグラウンドでの実行を管理する簡単な方法のひとつです。

あなたのマシンに`solと`いうユーザーがいると仮定して、`/etc/systemd/system/sol.service`というファイルを以下のように作成します。
```
[Unit]
Description=Solana Validator
After=network.target
Wants=solana-sys-tuner.service
StartLimitIntervalSec=0

[Service]
Type=simple
Restart=always
RestartSec=1
User=sol
LimitNOFILE=500000
LogRateLimitIntervalSec=0
Environment="PATH=/bin:/usr/bin:/home/sol/.local/share/solana/install/active_release/bin"
ExecStart=/home/sol/bin/validator.sh

[Install]
WantedBy=multi-user.target
```

ここで、`/home/sol/bin/validator.sh`を作成し、希望する`solana-validator`のコマンドラインを含めます。  `/home/sol/bin/validator.sh`を手動で実行すると、期待通りにバリデータが起動することを確認します。 `chmod +x /home/sol/bin/validator.sh `で実行可能にすることを忘れないでください。

サービスを開始するには以下のようにしてください。
```bash
$ sudo systemctl enable --now sol
```

### ログ
#### ログ出力チューニング

バリデータがログに出力するメッセージは、環境変数` RUST_LOG `で制御できます。 詳細は [ドキュメント](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) の `env_logger` Rust crate を参照してください。

ログの出力を減らすと、後で問題が発生したときにデバッグが難しくなることに注意してください。 チームにサポートを求める場合は、サポートを提供する前に、変更を元に戻して問題を再現する必要があります。

#### ログローテーション

`--log ~/solana-validator.log `で指定されるバリデータのログファイルは、時間の経過とともに非常に大きくなることがあるので、ログローテーションを設定することをお勧めします。

バリデータは` USR1 `シグナルを受け取ると再オープンします。これはログローテーションを有効にするための基本的なプリミティブです。

#### ログローテーションの使用

`logrotate`の設定例です。バリデータが"systemd"の`sol.service`というサービスとして動作していると仮定し、"/home/sol/solana-validator.log"にログファイルを書き込みます。
```bash
# Setup log rotation

cat > logrotate.sol <<EOF
/home/sol/solana-validator.log {
  rotate 7
  daily
  missingok
  postrotate
    systemctl kill -s USR1 sol.service
  endscript
}
EOF
sudo cp logrotate.sol /etc/logrotate.d/sol
systemctl restart logrotate.service
```

### ポートチェックを無効にして再起動を高速化
バリデータが正常に動作するようになったら、`solana-validator `のコマンドラインに `--no-port-check `フラグを追加することで、バリデータの再起動にかかる時間を短縮することができます。

### スナップショット圧縮を無効にしてCPU使用率を削減
他のバリデータにスナップショットを提供しない場合は、スナップショット圧縮を無効にしてCPU負荷を軽減することができますが、その場合はローカルのスナップショットストレージのディスク使用量が多少増えます。

`solana-validator `のコマンドライン引数に` -snapshot-compression none` 引数を追加して、バリデータを再起動します。

### SSDの消耗を抑えるために、アカウントデータベースにスワップにスピルオーバーするラムディスクを使用します。
If your machine has plenty of RAM, a tmpfs ramdisk ([tmpfs](https://man7.org/linux/man-pages/man5/tmpfs.5.html)) may be used to hold the accounts database

"tmpfs"を使うときは、定期的に"tmpfs"の容量が足りなくならないように、"swap"も設定しておく必要があります。

"300GB の tmpfs パーティション"と" 250GB のスワップパーティション"の使用を推奨します。

設定例:
1. `sudo mkdir /mnt/solana-accounts`
2. `"/etc/fstab"`に`tmpfs /mnt/solana-accounts tmpfs rw,size=300G,user=sol 0 0`という行を追加して、300GBのtmpfsパーティションを追加します(バリデータがユーザー"sol"で動作していることを想定しています)。  **注："/etc/fstab"の編集を誤ると、マシンが起動しなくなる可能性があります**。
3. 少なくとも250GBのスワップ領域を作成します
  - この後の説明で`SWAPDEV`の代わりに使用するデバイスを選びます。 理想的には、高速ディスク上の250GB以上のフリーディスクパーティションを選択してください。 もしない場合は、`sudo dd if=/dev/zero of=/swapfile bs=1MiB count=250KiB`でスワップファイルを作成し、`sudo chmod 0600 /swapfile` でパーミッションを設定し、`/swapfile `を以下の説明で` SWAPDEV` として使用します。
  - `sudo mkswap SWAPDEV `でスワップとして使用するためにデバイスをフォーマットします。
4. `"etc/fstab"` に "`SWAPDEV swap swap defaults 0 0"` を含むスワップファイルを追加します。
5. `"sudo swapon -a"` でスワップを有効にし、`"sudo mount /mnt/solana-accounts/"` で "tmpfs" をマウントします。
6. `"free -g"` でスワップが有効であること、`"mount"` で "tmpfs" がマウントされていることを確認します。

ここで、`solana-validator` のコマンドライン引数に` --accounts /mnt/solana-accounts` 引数を追加し、バリデータを再起動します。

### アカウントインデックス

クラスタの人口勘定科目の数が増えるにつれて、 口座データ RPC は、[`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) や [SPL-token-specific request](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) --のように、口座セット全体をスキャンします。 バリデータがこれらのリクエストをサポートする必要がある場合は、 `--account-index `パラメータを使用して 1 つ以上のメモリ内アカウントインデックスを有効にすることができます。 これは、アカウントを key フィールドでインデックス化することで RPC パフォーマンスを大幅に向上させるものです。 現在サポートしているパラメータ値は次のとおりです。

- `program-id`: それぞれの口座が所有するプログラムによってインデックスされます; [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts)
- `spl-token-mint`: トークンMintでインデックスされた各SPLトークンアカウント; [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate), および [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts) によって使用される。
- `spl-token-owner`: each SPL token account indexed by the token-owner address; used by [getTokenAccountsByOwner](developing/clients/jsonrpc-api.md#gettokenaccountsbyowner), and [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) requests that include an spl-token-owner filter.
