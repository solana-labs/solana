---
title: '"Exchange"にソラナを追加しよう'
---

このガイドでは、Solana のネイティブトークン SOL を暗号通貨取引所に追加する方法について説明します。

## ノードのセットアップ

ハイグレードなコンピューターやクラウドインスタンスに最低 2 つのノードを設定し、速やかに新しいバージョンにアップグレードし、付属の監視ツールでサービスの運用状況を把握することを強くお勧めします。

このセットアップはあなたを可能にします:

- データを取得するために"Solana mainnet-beta cluster"への信頼できるゲートウェイを保持した上で引き出しトランザクションを送信します。
- 履歴ブロックデータの保持量を完全に制御することができます
- 1 つのノードが失敗しても、サービスの可用性を維持するために。

Solana ノードは、高速ブロックと高い TPS を処理するために、比較的高いコンピューティング能力を必要としています。 具体的な要件については、[hardware recommendations](../running-validator/validator-reqs.md) を参照してください。

"Api ノード"を実行するには:

1. [Solana コマンドラインツールスイートをインストールします。](../cli/install-solana-cli-tools.md)
2. 以下のパラメータを使用してバリデータを開始します。

```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction-history \
  --limit-ledger-size \
  --trusted-validator <VALIDATOR_ADDRESS> \
  --no-untrusted-rpc
```

`--ledger` を希望する台帳の保存場所に、公開したいポートに `--rc-port` をカスタマイズします。

`--entrypoint` と `--expected-genesis-hash` パラメータはすべて、参加するクラスタに固有です。 [Mainnet Beta の現在のパラメータ](../clusters.md#example-solana-validator-command-line-2)

`--limit-ledger-size`パラメータでは、ノードがディスクに保持する台帳の数を[shred](../terminology.md#shred)で指定することができます。 このパラメータを指定しなかった場合、バリデータはディスクの空き容量がなくなるまで台帳全体を保持します。 デフォルト値は台帳ディスク使用量を 500GB 未満に保持します。 必要に応じて、`--limit-ledger-size` に引数を追加することで、ディスクの使用量を増やすことができます。 `solana-validator --help` for the default limit value used by `--limit-ledger-size`. カスタムリミット値を選択する方法の詳細は [こちら](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26) で確認できます。

1 つ以上のパラメータを指定する `--trusted-validator` は悪意のあるスナップショットからの起動を防ぐことができます。 [信頼されたバリデータで起動する値の詳細](../running-validator/validator-start.md#trusted-validators)

考慮すべき任意のパラメータ:

- `--private-rpc` が他のノードに公開されるのを防ぎます。
- `--rpc-bind-address` では、RPC ポートをバインドする別の IP アドレスを指定できます。

### 自動再起動とモニタリング

データの損失をできるだけ少なくするために、終了時に自動的に再起動するように各ノードを設定することをお勧めします。 "Solana ソフトウェア"を"systemd サービス"として実行することは選択肢の一つです。

監視のために、私たちは [`solana-watchtower`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md) を提供しています。これはあなたのバリデーターを監視し、 ` solana-validator` のプロセスの不健全性を検出することができます。 "Slack"、"Telegram"、"Discord"、"Twillio"を介して直接アラートを出すように設定することもできます。 詳細は`solana-watchtower --help` を実行してください。

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### 新規ソフトウェアリリースのお知らせ

私たちは頻繁に新しいソフトウェアをリリースしています(約 1 週間に 1 回のペース)。 新しいバージョンには、互換性のないプロトコルの変更が含まれていることがあり、ブロックの処理でエラーが発生しないように、タイムリーにソフトウェアを更新する必要があります。

あらゆる種類のリリースについての公式リリースアナウンス(通常および セキュリティ)は、[`#mb-announcement`](https://discord.com/channels/428295358100013066/669406841830244375) (`mb` stands for `mainnet-beta`)と呼ばれる Discord チャンネルを通じて通知されます。

ステーキングされたバリデータと同様に、取引所で運用されているバリデータも、通常のリリース発表から 1 ～ 2 営業日以内に、あなたの都合の良いように更新されることを期待しています。 セキュリティ関連のリリースでは、より緊急性の高い対応が必要になるかもしれません。

### Ledger Continuity

デフォルトでは、各ノードは信頼できるバリデータの 1 つが提供するスナップショットから起動します。 このスナップショットはチェーンの現在の状態を反映していますが、完全な履歴台帳を含んでいるわけではありません。 ノードの 1 つが終了して新しいスナップショットから起動すると、そのノードの台帳にギャップが生じることがあります。 この問題を防ぐためには、`solana-validator`コマンドに`--no-snapshot-fetch`パラメータを追加して、スナップショットの代わりに履歴台帳データを受け取るようにします。

最初のブート時に`--no-snapshot-fetch`パラメータを渡さないでください。なぜなら、ノードをジェネシスブロックからずっとブートすることはできないからです。 その代わり、最初にスナップショットから起動し、再起動時に`--no-snapshot-fetch`パラメータを追加します。

注意すべき点は、ネットワークの他の部分からノードが利用できる履歴台帳の量は、どの時点でも限られているということです。 運用開始後、バリデータが大幅にダウンした場合、ネットワークに追いつけなくなる可能性があります。 その場合、信頼できるバリデータから新しいスナップショットをダウンロードする必要があります。そうすると、バリデータは過去の台帳データとの間にギャップが生じ、それを埋めることができなくなります。

### バリデータポートのエクスポージャーを最小化する

バリデータは、他の Solana バリデータからの入力トラフィックに対して、さまざまな"UDP"と"TCP ポート"をオープンにする必要があります。 これは最も効率的な運用方法であり、強く推奨しますが、バリデータを制限して他の Solana バリデータからのインバウンドトラフィックのみを要求することも可能です。

最初に `--repa-only-mode` 引数を追加します。 この場合、バリデータは他のバリデータからのプッシュを受け取らない制限付きモードで動作することになり、代わりに他のバリデータに継続的にブロックを問い合わせる必要があります。 バリデータは、*Gossip*および*ServeR* ("serve repair") ポートを使って他のバリデータに UDP パケットを送信し、*Gossip*および*Repair*ポートで UDP パケットを受信するだけです。

_ゴシップ_ ポートは双方向であり、バリデータがクラスタの残りの部分と 接触することを許可します。 あなたのバリデータは*ServeR で*送信し、ネットワークの他の部分から新しいブロックを得るためのリペア要求を行っています。 あなたのバリデータは、他のバリデータから*Repair*ポートで修理の回答を受け取ることになります。

1 つまたは複数のバリデータからブロックのみを要求するバリデータをさらに制限する。 最初にそのバリデータの ID pubkey を決定します。 `--gossip-pull-validator PUBKEY --repa-validator PUBKEY` 引数は各 公開キーです。 この方法では、バリデータを追加するたびにリソースを消費することになるので、対象となるバリデータと相談した上で、慎重に行ってください。

これであなたのバリデータは、明示的にリストアップされたバリデータのみと、_Gossip_、_Repair_、*ServeR*の各ポートでのみ通信するようになるはずです。

## 入金アカウントの設定

Solana アカウントはチェーンの初期化を必要としません。 SOL が含まれるとそれらは存在します。 取引所の入金口座を設定するには、[ウォレットツール](../wallet-guide/cli.md) のいずれかを使用して Solana キーペアを生成します。

各ユーザーに固有の入金アカウントをご利用いただくことをお勧めします。

ソラナアカウントは、作成時とエポック毎に 1 回[賃料](developing/programming-model/accounts.md#rent)が発生しますが、SOL に 2 年分の賃料が含まれていれば、賃料を免除することができます。 入金口座の賃貸料免除の最小残高を見つけるには、[.`getMinimumBalanceForRentExemption` エンドポイント](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption) をクエリします。

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### オフラインアカウント

セキュリティを強化するために、1 つまたは複数のコレクションアカウントのキーをオフラインにしたい場合もあります。 その場合は、[オフラインの方法](../offline-signing.md)で SOL をホットアカウントに移動させる必要があります。

## 入金確認

ユーザーが SOL を取引所に入金したい場合は、適切な入金先に送金するように指示してください。

### ブロックのポーリング

取引所のすべての預金口座を追跡するには、"Solana API ノード"の"JSON-RPC サービス"を使用して、確認された各ブロックをポーリングし、関心のあるアドレスを検査します。

- どのブロックが利用可能かを確認するには、[`getConfirmedBlocks`リクエスト](developing/clients/jsonrpc-api.md#getconfirmedblocks)を送信し、start-slot パラメータとして既に処理した最後のブロックを渡します。

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```

すべてのスロットでブロックが生成されるわけではないので、整数の並びにずれが生じることがあります。

- 各ブロックの内容を [`getConfirmedBlock` リクエスト](developing/clients/jsonrpc-api.md#getconfirmedblock) で要求します。

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "postBalances": [
            2033973061360,
            218099990000,
            42000000003
          ],
          "preBalances": [
            2044973066360,
            207099990000,
            42000000003
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
              "11111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 1,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  0,
                  1
                ],
                "data": "3Bxs3zyH82bhpB8j",
                "programIdIndex": 2
              }
            ],
            "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
          },
          "signatures": [
            "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

`"preBalances"`および`"postBalances"`フィールドを使用すると、トランザクション全体を解析することなく、すべてのアカウントの残高変更を追跡することができます。 これらのフィールドには、[lamports](../terminology.md#lamport)内の各口座の開始と終了の残高を `accountKeys` リストにインデックス付けします。 たとえば、金利の場合の入金先が `47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi` の場合、この取引は 218099990000 - 207099990000 = 11000000000 lamports = 11 SOL の送金を表しています。

トランザクションの種類やその他の詳細情報が必要な場合は、RPC からブロックをバイナリ形式でリクエストし、[Rust SDK](https://github.com/solana-labs/solana)または[Javascript SDK](https://github.com/solana-labs/solana-web3.js)を使用して解析することができます。

### アドレス履歴

特定のアドレスのトランザクション履歴を照会することもできます。 これは一般的に、すべてのスロットのすべての預金アドレスを追跡するには有効な方法では*ありません*が、特定の期間にいくつかのアカウントを調査するには便利な場合があります。

- [`getConfirmedSignaturesForAddress2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) を"api ノード"にリクエストしてください:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedSignaturesForAddress2","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", {"limit": 3}]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
      "slot": 114
    },
    {
      "err": null,
      "memo": null,
      "signature": "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
      "slot": 112
    },
    {
      "err": null,
      "memo": null,
      "signature": "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6",
      "slot": 108
    }
  ],
  "id": 1
}
```

- 返された署名ごとに、 [`getConfirmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction) リクエストを送信してトランザクションの詳細を取得します。

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

// Result
{
  "jsonrpc": "2.0",
  "result": {
    "slot": 5,
    "transaction": {
      "message": {
        "accountKeys": [
          "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
          "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
          "11111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 1,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              0,
              1
            ],
            "data": "3Bxs3zyH82bhpB8j",
            "programIdIndex": 2
          }
        ],
        "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
      },
      "signatures": [
        "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
      ]
    },
    "meta": {
      "err": null,
      "fee": 5000,
      "postBalances": [
        2033973061360,
        218099990000,
        42000000003
      ],
      "preBalances": [
        2044973066360,
        207099990000,
        42000000003
      ],
      "status": {
        "Ok": null
      }
    }
  },
  "id": 1
}
```

## 引き出しを送信

ユーザーからの SOL の引き出し要求に対応するためには、Solana の転送トランザクションを生成し、それを"api ノード"に送り、自分のクラスターに転送する必要があります。

### 同期

Solana クラスタに同期転送を送信すると、クラスタによって転送が成功したことを確認できます。

Solana のコマンドラインツールは、 `solana transfer`のトランザクションを"生成"、"送信"、および"確認"する簡単なコマンドを提供します。 デフォルトでは、このメソッドはクラスタによってトランザクションが完了するまで待機し、"stderr" の進捗状況を追跡します。 トランザクションが失敗した場合、トランザクションエラーが報告されます。

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --allow-unfunded-recipient --keypair <KEYPAIR> --url http://localhost:8899
```

[Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) は、"JS エコシステム"に似たアプローチを提供します。 `SystemProgram` を使用して、転送トランザクションを構築し。`sendAndConfirmTransaction`メソッドを使用して送信します。

### 非同期

柔軟性を高めるため、非同期で引き出し転送を行うことができます。 このような場合、トランザクションが成功し、クラスタによって確定されたことを確認するのは利用者の責任です。

**注意:** 各トランザクションには、 [最近の ブロックハッシュ](developing/programming-model/transactions.md#blockhash-format) が含まれており、 はその生計を示します。 クラスタで確認されていない、または確定されていないと思われる引き出し転送を再試行する前に、このブロックハッシュの有効期限が切れるまで待つことが**重要**です。 そうでなければ、二重支払いする危険性があります。 詳細は [ブロックハッシュの有効期限](#blockhash-expiration) をご覧ください。

まず、 [`getFees` エンドポイント](developing/clients/jsonrpc-api.md#getfees)または CLI コマンドを使用して、最近のブロックハッシュを取得します。

```bash
ソラナ手数料 --url http://localhost:8899
```

コマンドラインツールでは、非同期に転送を行うために`--no-wait`引数を渡し、`--blockhash`引数で最近の"Blockhash"を含めます。

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --allow-unfunded-recipient --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

トランザクションを手動でビルド、署名、シリアライズすることもできます。 そして、JSON-RPC [`sendTransaction` エンドポイント](developing/clients/jsonrpc-api.md#sendtransaction) を使用してクラスタにオフにします。

#### トランザクションの確認 & ファイナリティ

[`getSignatureStatuss` "JSON-RPC エンドポイント"](developing/clients/jsonrpc-api.md#getsignaturestatuses) を使用してトランザクションのステータスを取得します。 `"confirmations"`フィールドでは、トランザクションが処理されてから[確認済みのブロック](../terminology.md#confirmed-block)がいくつ経過したかが報告されます。 `"confirmations": null`の場合は、[ファイナライズされています](../terminology.md#finality)。

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatuses", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]}' http://localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 72,
        "confirmations": 10,
        "err": null,
        "status": {
          "Ok": null
        }
      },
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        }
      }
    ]
  },
  "id": 1
}
```

#### ブロックハッシュの有効期限

You can check whether a particular blockhash is still valid by sending a [`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash) request with the blockhash as a parameter. If the response value is `null`, the blockhash is expired, and the withdrawal transaction using that blockhash should never succeed.

### 引き出しのためのユーザーが提供するアカウントアドレスの検証

出金は元に戻すことができないため、ユーザーの資金が誤って失われるのを防ぐために、出金を承認する前に、ユーザーが提供したアカウントのアドレスを検証するのは良い方法かもしれません。

#### Basic verfication

Solana addresses a 32-byte array, encoded with the bitcoin base58 alphabet. This results in an ASCII text string matching the following regular expression:

```
[1-9A-HJ-NP-Za-km-z]{32,44}
```

This check is insufficient on its own as Solana addresses are not checksummed, so typos cannot be detected. To further validate the user's input, the string can be decoded and the resulting byte array's length confirmed to be 32. However, there are some addresses that can decode to 32 bytes despite a typo such as a single missing character, reversed characters and ignored case

#### Advanced verification

Due to the vulnerability to typos described above, it is recommended that the balance be queried for candidate withdraw addresses and the user prompted to confirm their intentions if a non-zero balance is discovered.

#### Valid ed25519 pubkey check

Solana の通常のアカウントのアドレスは、256 ビットの"ed25519 公開キー"を"Base58"でエンコードした文字列です。 すべてのビットパターンが"ed25519 曲線"の有効な公開キーであるとは限らないため、ユーザーが提供するアカウントアドレスが少なくとも正しい"ed25519 公開キー"であることを保証することが可能です。

#### Java

ここでは、ユーザーが提供したアドレスを、有効な"ed25519 公開キー"として検証する Java の例を示します。

以下のコードサンプルは、"Maven"を使用していることを前提としています。

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>spring</id>
    <url>https://repo.spring.io/libs-release/</url>
  </repository>
</repositories>

...

<dependencies>
  ...
  <dependency>
      <groupId>io.github.novacrypto</groupId>
      <artifactId>Base58</artifactId>
      <version>0.1.3</version>
  </dependency>
  <dependency>
      <groupId>cafe.cryptography</groupId>
      <artifactId>curve25519-elisabeth</artifactId>
      <version>0.1.0</version>
  </dependency>
<dependencies>
```

```java
import io.github.novacrypto.base58.Base58;
import cafe.cryptography.curve25519.CompressedEdwardsY;

public class PubkeyValidator
{
    public static boolean verifyPubkey(String userProvidedPubkey)
    {
        try {
            return _verifyPubkeyInternal(userProvidedPubkey);
        } catch (Exception e) {
            return false;
        }
    }

    public static boolean _verifyPubkeyInternal(String maybePubkey) throws Exception
    {
        byte[] bytes = Base58.base58Decode(maybePubkey);
        return !(new CompressedEdwardsY(bytes)).decompress().isSmallOrder();
    }
}
```

## SPL トークンスタンダードをサポート

[SPL トークン](https://spl.solana.com/token) は Solana ブロックチェーンでのラップ/合成のトークンの作成と交換の標準です。

SPL Token のワークフローは、ネイティブの SOL トークンのワークフローと似ていますが、いくつかの違いがありますので、このセクションで説明します。

### Token Mints

Each _type_ of SPL Token is declared by creating a _mint_ account. このアカウントには、供給量や小数点以下の数、造幣局を管理する様々な権限など、トークンの特徴を表すメタデータが保存されています。 それぞれの SPL トークンのアカウントは、関連するミントを参照し、そのタイプの SPL トークンとのみやり取りすることができます。

### `spl-token` CLI ツールのインストール

SPL トークンのアカウントは、`spl-token`コマンドラインユーティリティを使って照会したり変更したりします。 このセクションで提供される例は、ローカルシステムにインストールされていることに依存します。

`"spl-token"`は Rust の["crates.io"](https://crates.io/crates/spl-token)から Rust の`"cargo"`コマンドラインユーティリティを介して配布されています。 `"cargo"`の最新バージョンは、あなたのプラットフォーム用の便利なワンライナーを使って [rustup.rs](https://rustup.rs)でインストールできます。 `cargo` がインストールされると、次のコマンドで `spl-token` を取得できます。

```
cargo install spl-token-cli
```

インストールされているバージョンを確認して確認できます。

```
spl-token --version
```

結果は次のようなものになるはずです。

```text
spl-token-cli 2.0.1
```

### アカウント作成

SPL トークンアカウントには、"System Program" のネイティブアカウントではない追加の要件が含まれています。

1. SPL トークンアカウントは、トークンを預ける前に作成する必要があります。 トークンアカウントは、`spl-token create-account`コマンドで明示的に作成することも、`spl-token transfer --fund-recipient ...`コマンドで暗黙的に作成することもできます。
1. SPL Token アカウントは、その存続期間中、[家賃免除](developing/programming-model/accounts.md#rent-exemption)でなければならないため、アカウント作成時に少量のネイティブ SOL トークンを預ける必要があります。 SPL Token v2 アカウントの場合、この金額は 0.00203928 SOL(2,039,280 lamports) です。

#### コマンドライン

以下のプロパティを持つ SPL トークンアカウントを作成するには次のようになります。

1. 与えられたミントに関連付けられていること。
1. 資金調達アカウントのキーペアによって所有されていること。

```
spl-token create-account <TOKEN_MINT_ADDRESS>
```

#### 例

```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

または、特定のキーペアで SPL トークンアカウントを作成するには次のようになります。

```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### アカウントの残高の確認

#### コマンドライン

```
spl-token balance <TOKEN_ACCOUNT_ADDRESS>
```

#### 例

```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### トークンの転送

送金元の口座は、送金額が入っている実際のトークンアカウントです。

しかし、受取人のアドレスは通常のウォレットのアカウントで構いません。 指定されたミントに関連付けられたトークンアカウントがそのウォレットにまだ存在しない場合、`--fund-recipient` 引数が指定されていれば、送金によってトークンアカウントが作成されます。

#### コマンドライン

```
spl-token transfer <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### 例

```
$ spl-token transfer 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Transfer 1 tokens
  Sender: 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN
  Recipient: 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### デポジット

各 `(user, mint) `ペアはチェーン上に個別のアカウントを必要とするため、取引所は事前にトークン アカウントのバッチを作成し、リクエストに応じてユーザに割り当てることを推奨します。 これらのアカウントはすべて、取引所が管理するキーペアで所有されるべきです。

入金取引の監視は、上述の["ブロックポーリング法"](#poll-for-blocks)に従うべきです。 各新しいブロックは、成功したトランザクションのためにスキャンされなければなりません。 SPL トークンの [Transfer](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92) または [Transfer2](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) 命令を発行し、ユーザーアカウントを参照し、[トークンのアカウント残高](developing/clients/jsonrpc-api.md#gettokenaccountbalance)の更新を照会します。

[考慮事項](https://github.com/solana-labs/solana/issues/12318) は SPL トークン残高転送を含む `preBalance` と `postBalance` トランザクション状況メタデータフィールドを延長するために行われています。

### 引き出し

ユーザーが提供する引き出しアドレスは、通常の SOL の引き出しに使用されるアドレスと同じでなければなりません。

取引所は、出金を実行する前に[転送](#token-transfers)としてアドレスを確認する必要があります。 [上記](#validating-user-supplied-account-addresses-for withdrawalals)の通りです。

引き出しアドレスから、正しいミントの関連トークンアカウントが決定され、そのアカウントに送金が発行されます。 なお、関連するトークン口座がまだ存在しない可能性もありますが、その場合は取引所がユーザーに代わって口座に入金する必要があります。 For SPL Token v2 accounts, funding the withdrawal account will require 0.00203928 SOL (2,039,280 lamports).

Template `spl-token transfer` command for a withdraw:

```
$ spl-token transfer --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### その他の考慮事項

#### 凍結権限

For regulatory compliance reasons, an SPL Token issuing entity may optionally choose to hold "Freeze Authority" over all accounts created in association with its mint. これにより、特定のアカウントの資産を自由に[凍結](https://spl.solana.com/token#freezing-accounts)し、解凍されるまでそのアカウントを使用できなくすることができます。 この機能を使用している場合、"凍結権限者の公開キー"は SPL トークンのミントアカウントに登録されます。

## インテグレーションのテスト

"mainnet-beta"の本番環境に移行する前に、必ず Solana の devnet および testnet[クラスタ](../clusters.md)でワークフロー全体をテストしてください。 "devnet"は最もオープンで柔軟性が高く、初期の開発に適しており、"testnet"はより現実的なクラスタ構成を提供します。 Both devnet and testnet support a faucet, run `solana airdrop 1` to obtain some devnet or testnet SOL for developement and testing.
