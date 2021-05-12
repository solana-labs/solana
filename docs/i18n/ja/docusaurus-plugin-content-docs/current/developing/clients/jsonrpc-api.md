---
title: JSON RPC API
---

Solana ノードは [JSON-RPC 2.0](https://www.jsonrpc.org/specification) 仕様を使用して HTTP リクエストを受け付けます。

JavaScript アプリケーションの中で Solana ノードを操作するには、RPC メソッドの便利なインターフェイスを提供する[solana-web3.js](https://github.com/solana-labs/solana-web3.js)ライブラリを使用します。

## RPCHTTP エンドポイント

**Default port:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## RPC PubSub WebSocket エンドポイント

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## メソッド

- [getAccountInfo](jsonrpc-api.md#getaccountinfo)
- [getBalance](jsonrpc-api.md#getbalance)
- [getBlockCommitment](jsonrpc-api.md#getblockcommitment)
- [getBlockTime](jsonrpc-api.md#getblocktime)
- [getClusterNodes](jsonrpc-api.md#getclusternodes)
- [getConfirmedBlock](jsonrpc-api.md#getconfirmedblock)
- [getConfirmedBlocks](jsonrpc-api.md#getconfirmedblocks)
- [getConfirmedBlocksWithLimit](jsonrpc-api.md#getconfirmedblockswithlimit)
- [getConfirmedSignaturesForAddress](jsonrpc-api.md#getconfirmedsignaturesforaddress)
- [getConfirmedSignaturesForAddress2](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [getConfirmedTransaction](jsonrpc-api.md#getconfirmedtransaction)
- [getEpochInfo](jsonrpc-api.md#getepochinfo)
- [getEpochSchedule](jsonrpc-api.md#getepochschedule)
- [getFeeCalculatorForBlockhash](jsonrpc-api.md#getfeecalculatorforblockhash)
- [getFeeRateGoveral](jsonrpc-api.md#getfeerategovernor)
- [取得手数料](jsonrpc-api.md#getfees)
- [getFirstAvailableBlock](jsonrpc-api.md#getfirstavailableblock)
- [getGenesisHash](jsonrpc-api.md#getgenesishash)
- [getHealth](jsonrpc-api.md#gethealth)
- [getIdentity](jsonrpc-api.md#getidentity)
- [getInflationGoveral](jsonrpc-api.md#getinflationgovernor)
- [getInflationRate](jsonrpc-api.md#getinflationrate)
- [getLargestAccounts](jsonrpc-api.md#getlargestaccounts)
- [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
- [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [getMultipleAccounts](jsonrpc-api.md#getmultipleaccounts)
- [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
- [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
- [getRecentPerformanceSamples](jsonrpc-api.md#getrecentperformancesamples)
- [getSignatureStatuses](jsonrpc-api.md#getsignaturestatuses)
- [getSlot](jsonrpc-api.md#getslot)
- [getSlotLeader](jsonrpc-api.md#getslotleader)
- [getStakeActivation](jsonrpc-api.md#getstakeactivation)
- [getSupply](jsonrpc-api.md#getsupply)
- [getTransactionCount](jsonrpc-api.md#gettransactioncount)
- [getVersion](jsonrpc-api.md#getversion)
- [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
- [minimumLedgerSlot](jsonrpc-api.md#minimumledgerslot)
- [requestAirdrop](jsonrpc-api.md#requestairdrop)
- [トランザクションを送信](jsonrpc-api.md#sendtransaction)
- [simulateTransaction](jsonrpc-api.md#simulatetransaction)
- [setLogFilter](jsonrpc-api.md#setlogfilter)
- [validatorExit](jsonrpc-api.md#validatorexit)
- [サブスクリプション Web ソケット](jsonrpc-api.md#subscription-websocket)
  - [accountSubscribe](jsonrpc-api.md#accountsubscribe)
  - [アカウント登録解除](jsonrpc-api.md#accountunsubscribe)
  - [logsSubscribe](jsonrpc-api.md#logssubscribe)
  - [logsUnsubscribe](jsonrpc-api.md#logsunsubscribe)
  - [programSubscribe](jsonrpc-api.md#programsubscribe)
  - [programUnsubscribe](jsonrpc-api.md#programunsubscribe)
  - [signatureSubscribe](jsonrpc-api.md#signaturesubscribe)
  - [signatureUnsubscribe](jsonrpc-api.md#signatureunsubscribe)
  - [slotSubscribe](jsonrpc-api.md#slotsubscribe)
  - [slotUnsubscribe](jsonrpc-api.md#slotunsubscribe)

## 不安定なメソッド

不安定な方法では、パッチリリースの破損した変更が見られる可能性があり、永続的にサポートされていない可能性があります。

- [getTokenAccountBalance](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [getTokenLargestAccounts](jsonrpc-api.md#gettokenlargestaccounts)
- [getTokenSupply](jsonrpc-api.md#gettokensupply)

## 要求書式設定

JSON-RPC リクエストを行うには、 `Content-Type: application/json` ヘッダーを持つ HTTP POST リクエストを送信します。 JSON リクエストデータは 4 つのフィールドを含める必要があります:

- `jsonrpc: <string>`, `"2.0"` に設定
- `id: <number>`, 一意のクライアントで生成された識別整数.
- `method: <string>`, 呼び出されるメソッドを含む文字列
- `パラメータ: <array>`, 順序付けられたパラメータ値の JSON 配列

Curl を使用した例：

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"
    ]
  }
'
```

レスポンス出力は以下のフィールドを持つ JSON オブジェクトになります:

- `jsonrpc: <string>`, 要求仕様に一致する
- `id: <number>`, 要求識別子と一致する
- `結果: <array|number|object|string>`, 要求されたデータまたは成功の確認

JSON-RPC リクエストオブジェクトの配列を単一の POST のデータとして送信することで、リクエストをバッチで送信できます。

## 定義

- ハッシュ: データの塊の SHA-256 ハッシュ
- Pubkey: Ed25519 キーペアの公開鍵。
- トランザクション: クライアントのキーペアによって署名された Solana 命令のリスト。
- 署名: 命令を含むトランザクションのペイロード・データの Ed25519 署名 これはトランザクションを識別するために使用できます。

## 状態コミットメントの設定

プリフライトチェックとトランザクション処理のために、Solana ノードはクライアントによって設定されたコミットメント要件に基づいてクエリするバンク状態を選択します。 コミットメントは、その時点でブロックがどのように最終化されたかを記述しています。 台帳の状態を照会する際には、進捗状況を報告するために低いレベルのコミットメントを使用し、状態がロールバックされないようにするために高いレベルを使用することをお勧めします。

コミットメントの降順(最も確定が少ない) で、クライアントは以下を指定することができます:

- `"max"` - ノードは、クラスタの超過半数によって最大ロックアウトに達したことが確認された最新のブロックを照会します（クラスタがこのブロックを確定したと認識したことを意味します）。
- `"root"` - ノードは、このノードのロックアウトが最大に達した直近のブロックを問い合わせます。 つまりノードはこのブロックを確定として認識しています
- `"singleゴシップ"` - クラスターの過半数によって投票された最新のブロックをノードが照会します。
  - ゴシップやリプレイによる投票を取り入れています。
  - あるブロックの子孫の票はカウントせず、そのブロックへの直接票のみをカウントします。
  - この確認レベルは、リリース 1.3 以降の"optimistic confirmation"を保証するものでもあります。
- `"recent"` -ノードはその最新のブロックを照会します。 なお、ブロックが完全でない場合もあります。

多数の依存性のあるトランザクションを連続して処理する場合は、速度とロールバックの安全性のバランスがとれた`singleGossip`コミットメントを使用することが推奨されます。 完全な安全性を求めるのであれば、`"max `コミットメントを使用することをお勧めします。

#### 例

コミットメントパラメータは `params` 配列の最後の要素として含める必要があります。

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "max"
      }
    ]
  }
'
```

#### デフォルト:

コミットメント設定が提供されていない場合、ノードはデフォルトで `"max"` コミットメントになります

Bank 状態をクエリするメソッドだけがコミットメントパラメータを受け入れます。 これらは以下の API リファレンスに示されています。

#### RpcResponse 構造

コミットメントパラメータを取る多くのメソッドは、2 つの部分から構成された RpcResponse JSON オブジェクトを返します。

- `context` : 操作が評価された `スロット` フィールドを含む RpcResponseContext JSON 構造。
- `value` : 操作自体が返す値。

## ヘルスチェック

JSON RPC API ではありませんが。 RPC の HTTP エンドポイントの `GET/health` は、ロードバランサや他のネットワークインフラストラクチャで使用するためのヘルスチェックメカニズムを提供します。 このリクエストは、以下の条件に基づいて、常に"ok" または "behinder" のボディを持つ HTTP 200 OK レスポンスを返します。

1. `solana-validator`に 1 つ以上の`--trusted-validator`引数が与えられた場合、ノードが最高信頼度のバリデータの`HEALTH_CHECK_SLOT_DISTANCE`スロット内にある場合は "ok "が、そうでない場合は "behind "が返されます。
2. 信頼できるバリデータが提供されていない場合、"ok" は常に返されます。

## JSON RPC API リファレンス

### getAccountInfo

提供されたパブキーのアカウントに関連するすべての情報を返します。

#### パラメータ:

- `<string>`-base-58 エンコード文字列として、クエリするトークンアカウントのパブキー
- `<object>` - (任意) 次のオプションの 項目を含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "base58" は 128 バイト未満のアカウントデータに制限されています。 "base64" は base64 エンコードされたデータを任意のサイズの Account データに対して返します。 "base64+zstd" は、 [Zstandard](https://facebook.github.io/zstd/) を使用してアカウントデータを圧縮し、結果を base64 エンコードします。 "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されていてパーサが見つからない場合、フィールドは "base64" エンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。
  - (オプション) `dataSlice: <object>` - 指定された `オフセット: <usize>` と `長さ: <usize>` フィールド; "base58", "base64" または "base64+zstd" エンコーディングでのみ使用できます。

#### 結果:

結果は `値` 以下に等しい RpcResponse JSON オブジェクトになります:

- `<null>` - リクエストされたアカウントが存在しない場合
- `<object>` - そうでなければ、以下を含む JSON オブジェクト：
  - `lamports: <u64>`, このアカウントに割り当てられたラムポートの数, u64 として
  - `owner: <string>`, このアカウントに割り当てられたプログラムの base-58 エンコードされた Pubkey
  - `data:<[string, encoding]|object>`>, アカウントに関連するデータで、エンコードされたバイナリデータまたはエンコーディングパラメータ依存の JSON 形式 `{<program>: <state>}`
  - `executable: <bool>`, アカウントにプログラム \(および厳密には読み取り専用\) が含まれているかを示す boolean
  - `rentEpoch: <u64>`, u64 として、このアカウントが賃貸借義務を負うエポック

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
      {
        "encoding": "base58"
      }
    ]
  }
'
```

応答:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
        "base58"
      ],
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

応答:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": {
        "nonce": {
          "initialized": {
            "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
            "blockhash": "3xLP3jK6dVJwpeGeTDYTwdDK3TKchUf1gYYGHa4sF3XJ",
            "feeCalculator": {
              "lamportsPerSignature": 5000
            }
          }
        }
      },
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

### getBalance

指定された Pubkey のアカウントの残高を返します。

#### パラメータ

- `<string>` -base-58 文字列でエンコードされたクエリする為のアカウント所有者の公開キー
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `RpcResponse<u64>` - `value` フィールドを持つ RpcResponse JSON オブジェクト

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": { "context": { "slot": 1 }, "value": 0 },
  "id": 1
}
```

### getBlockCommitment

特定のブロックのコミットメントを返します。

#### パラメータ:

- `<u64>` - スロットで識別されるブロック

#### 結果:

結果フィールドは以下のような JSON オブジェクトになります。

- `commitation` - 以下のいずれかを含むコミットメント。
  - `<null>` - 不明なブロック
  - `<array>` -commitment, 0 から`MAX_LOCKOUT_HISTORY` + 1 までの各深度において、ブロックに投票したクラスタの出資額（lamports）を記録した u64 整数の配列
- `totalStake` - 現在の時代のランポートにおける総積額

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "commitment": [
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 10, 32
    ],
    "totalStake": 42
  },
  "id": 1
}
```

### getBlockTime

確認済みブロックの推定生産時間を返します。

各バリデータは、特定のブロックの投票にタイムスタンプを追加することで、UTC 時間を台帳に報告します。 要求されたブロックタイムは、台帳に記録された一連の最近のブロックの中で、投票タイムスタンプのステーキング荷重平均から計算されます。

スナップショットから起動しているか、台帳のサイズを制限しているノード(古い スロットのパージによって) は、最も低いルート + `TIMESTAMP_SLOT_RANGE` より下のブロックに対してゼロタイムスタンプを返します。 この履歴データを持つことに興味を持っているユーザーはジェネシスから構築されたノードをクエリし、台帳全体を保持する必要があります。

#### パラメータ:

- `<u64>` - スロットで識別されるブロック

#### 結果:

- `<i64>` - 制作時間を Unix タイムスタンプとして推定する (Unix エポックから数秒)
- `<null>` - タイムスタンプはこのブロックでは使用できません

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockTime","params":[5]}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### getClusterNodes

クラスタに参加しているすべてのノードに関する情報を返します。

#### パラメータ:

該当なし

#### 結果:

Result フィールドは JSON オブジェクトの配列であり、それぞれに以下のサブフィールドがあります。

- `pubkey: <string>` - base-58 でエンコードされた文字列のノードの公開キー
- `ゴシップ: <string>` - ノードのゴシップネットワーク アドレス
- `tpu: <string>` - ノードの TPU ネットワークアドレス
- `rpc: <string>|null` - ノードの JSON RPC ネットワークアドレス、JSON RPC サービスが有効でない場合 `null`
- `version: <string>|null` - ノードのソフトウェアバージョン、バージョン情報が利用できない場合は `null`

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "gossip": "10.239.6.48:8001",
      "pubkey": "9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ",
      "rpc": "10.239.6.48:8899",
      "tpu": "10.239.6.48:8856",
      "version": "1.0.0 c375ce1f"
    }
  ],
  "id": 1
}
```

### getConfirmedBlock

台帳内の確認済みブロックに関する身元と取引情報を返します。

#### パラメータ:

- `<u64>` - スロット, u64 integer
- `<string>` - "json", "jsonParsed", "base58" (_slow_), "base64". パラメータが指定されていない場合、デフォルトのエンコーディングは "json" です。 "jsonParsed" エンコーディングは、プログラム固有の命令パーサを使用して `transaction.message.instructions` リストに、より人間が読みやすく明示的なデータを返します。 If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).

#### 結果:

結果フィールドは、次のフィールドを持つオブジェクトになります。

- `<null>` - 指定されたブロックが確認されていない場合
- `<object>` - ブロックが確認された場合、次のフィールドを持つオブジェクト:
  - `blockhash: <string>` - base-58 エンコードされた文字列として
  - `previousBlockhash: <string>` - base-58 エンコードされた文字列として、このブロックの親のブロックハッシュ; 元帳のクリーンアップのために親ブロックが使用できない場合、このフィールドは「1111111111111111111111111111111111」を返します。
  - `parentSlot: <u64>` - このブロックの親のスロットインデックス
  - `transactions: <array>` - 以下を含む JSON オブジェクトの配列。
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
    - `<object>` -トランザクション・ステータス・メタデータ・オブジェクトで、`Null`または
      - `err: <object | null>` -トランザクションが失敗した場合はエラー、トランザクションが成功した場合は NULL です。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - u64 整数としてこのトランザクションに課金された手数料
      - `preBalances: <array>` - トランザクションが処理される前の u64 アカウント残高の配列
      - `postBalances: <array>` - トランザクションが処理された後の u64 アカウント残高の配列
      - `innerInstructions: <array|undefined>` -[内部命令のリスト](#inner-instructions-structure)、またはこのトランザクションで内部命令の記録がまだ有効になっていない場合は省略されます。
      - `logMessages: <array>` -ログメッセージの文字列の配列、またはこのトランザクションでログメッセージの記録がまだ有効でない場合は省略されます。
      - DEPRECATED:` status:<object>` - トランザクションステータス
        - `"Ok": <null>` - トランザクションが成功しました
        - `"Err": <ERR>` - TransactionError でトランザクションが失敗しました
  - `rewards: <array>` - 以下を含む JSON オブジェクトの配列。
    - `pubkey: <string>` - 報酬を受け取ったアカウントの公開鍵（Base-58 でエンコードされた文字列）。
    - `lamports: <i64>`- アカウントにクレジットまたはデビットされた i64 のランポート数。
    - `postBalance: <u64>` -報酬適用後のランポートのアカウント残高
    - `rewardType: <string|undefined>` -報酬の種類 "fee"、"rent"、"voting"、"staking"
  - `blockTime: <i64 | null>` - 推定生産時間 Unix タイムスタンプ(Unix エポックからの秒数) 利用できない場合は null

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
              "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
              "SysvarS1otHashes111111111111111111111111111",
              "SysvarC1ock11111111111111111111111111111111",
              "Vote111111111111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 3,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [1, 2, 3, 0],
                "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
                "programIdIndex": 4
              }
            ],
            "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
          },
          "signatures": [
            "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "status": {
            "Ok": null
          }
        },
        "transaction": [
          "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
          "base64"
        ]
      }
    ]
  },
  "id": 1
}
```

#### トランザクション構成

取引は、他のブロックチェーンでの取引とは全く異なります。 要確認： [Anatomy of a Transaction](developing/programming-model/transactions.md) Solana でのトランザクションについて学びます。

トランザクションの JSON 構造は以下のように定義されます:

- `signatures: <array[string]>` - トランザクションに適用される base-58 でエンコードされた署名のリストです。 リストは常に長さ `message.header.numRequiredSignatures` で、空ではありません。 Index `i` の署名は、 `message.account_keys` の index `i` の公開キーに対応します。 最初のものは [トランザクション Id](../../terminology.md#transaction-id) として使用されます。
- `message: <object>` - トランザクションの内容を定義します。
  - `accountKeys: <array[string]>` - トランザクションで使用される base-58 でエンコードされた公開キーのリスト。 最初の `message.header.numRequiredSignatures` 公開キーはトランザクションに署名する必要があります。
  - `header: <object>` - 取引で必要とされるアカウントの種類と署名の詳細。
    - `numRequiredSignatures: <number>` - トランザクションを有効にするために必要な署名の総数。 署名は、 `message.account_keys` の最初の `numRequiredSignatures` に一致する必要があります。
    - `numReadonlySignedAccounts: <number>` - 署名されたキーの最後の `numReadonlySignedAccounts` は読み取り専用のアカウントです。 プログラムは、1 つの PoH エントリ内で読み取り専用アカウントをロードする複数のトランザクションを処理することができます。 しかし、クレジットカードやデビットカード、口座データの変更は許可されていません。 同じ読み書き口座を対象とするトランザクションは順次評価されます。
    - `numReadonlyUnsignedAccounts: <number>` - 署名されていないキーの最後の `numReadonlyUnsignedAccounts` は読み取り専用アカウントです。
  - `recentBlockhash: <string>` - トランザクションの重複を防ぎ、トランザクションの寿命を延ばすために使用される台帳内の最近のブロックの base-58 でエンコードされたハッシュ。
  - `命令: <array[object]>` - 全てが成功した場合、順番に実行され、1 つのアトミックトランザクションでコミットされるプログラム命令のリスト。
    - `programIdIndex: <number>` - この命令を実行するプログラムアカウントを示す `message.accountKeys` 配列へのインデックス。
    - `accounts: <array[number]>` - プログラムに渡す口座を示す `message.accountKeys` 配列に注文されたインデックスのリスト。
    - `data: <string>` - base-58 文字列でエンコードされたプログラム入力データ。

#### インナーインストラクション構造

Solana ランタイムは、トランザクション処理中に呼び出されたクロスプログラム命令を記録し、トランザクション命令ごとにチェーン上で実行されたものの透明性を向上させます。 呼び出された命令は、元のトランザクション命令によってグループ化され、処理の順にリストされます。

内部命令の JSON 構造は、次の構造内のオブジェクトのリストとして定義されています。

- `index: number` -内部命令(s) が発生したトランザクション命令のインデックス。
- `instructions: <array[object]>` - 単一のトランザクション命令中に呼び出された内部プログラム命令の順序リスト。
  - `programIdIndex: <number>` - この命令を実行するプログラムアカウントを示す `message.accountKeys` 配列へのインデックス。
  - `accounts: <array[number]>` - プログラムに渡す口座を示す `message.accountKeys` 配列に注文されたインデックスのリスト。
  - `data: <string>` - base-58 string でエンコードされたプログラム入力データ。

### getConfirmedBlocks

2 つのスロット間の確認済みブロックのリストを返します

#### パラメータ:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - (任意) end_slot, as u64 integer

#### 結果:

結果フィールドは、`start_slot`から`end_slot`（指定されている場合）までの間に確認されたブロック、または最新の確認されたブロックを含む、u64 整数の配列となります。 許容最大範囲は 500,000 スロットです。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": [5, 7, 8, 9, 10], "id": 1 }
```

### getConfirmedBlocksWithLimit

指定されたスロットから始まる確認されたブロックのリストを返します

#### パラメータ:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer

#### 結果:

Result フィールドは、確認されたブロックを `start_slot` から `limit` ブロックまでリストする u64 整数の配列になります。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getConfirmedSignaturesForAddress

**DEPRECATED: 代わりに getConfirmedSignaturesForAddress2 を使用してください**

指定されたスロット範囲内のアドレスを含むトランザクションの確認済み署名のリストを返します。 許可されている最大範囲は 10,000 スロットです

#### パラメータ:

- `<string>` - base-58 エンコードされた文字列としてのアカウント アドレス
- `<u64>` - 含まれるスロットを開始
- `<u64>` - 含まれるスロットを終了

#### 結果:

結果フィールドは以下の配列になります:

- `<string>` - base-58 エンコードされた文字列としてのトランザクション署名

署名は、確認されたスロット、最低スロットから最高スロットに基づいて発注されます。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress",
    "params": [
      "6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC",
      0,
      100
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": [
    "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
    "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
    "4LQ14a7BYY27578Uj8LPCaVhSdJGLn9DJqnUJHpy95FMqdKf9acAhUhecPQNjNUy6VoNFUbvwYkPociFSf87cWbG"
  ],
  "id": 1
}
```

### getConfirmedSignaturesForAddress2

提供された署名または直近に確認されたブロックから、アドレスを逆方向に含むトランザクションの確認済みの署名を返します。

#### パラメータ:

- `<string>` - base-58 エンコードされた文字列としてのアカウント アドレス
- `<object>` - (任意) 次のフィールドを含む設定オブジェクト。
  - `limit: <number>` - (任意) 返す最大トランザクション署名数(1 から 1,000、デフォルト：1,000)
  - `before: <string>` - (任意) このトランザクション署名から逆方向の検索を開始します。 指定されていない場合は、最も高い確認されたブロックの上から検索が開始されます。
  - `until: <string>` - (任意) このトランザクション署名に達するまで検索します。

#### 結果:

Result フィールドは、トランザクション署名情報の配列を最新から最古のトランザクションに注文します。

- `<object>`
  - `signature: <string>` - base-58 エンコードされた文字列としてのトランザクション署名
  - `slot: <u64>` - トランザクションを含むブロックを含むスロット
  - `err: <object | null>` - トランザクションが成功した場合は null。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - トランザクションに関連付けられたメモ メモがない場合は null。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress2",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
      }
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114
    }
  ],
  "id": 1
}
```

### getConfirmedTransaction

確認済みトランザクションのトランザクションの詳細を返します

#### パラメータ:

- `<string>` - transaction signature as base-58 encoded string N encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields)
- `<string>` - (optional)返されるトランザクションのエンコーディング、"json", "jsonParsed", "base58" (_slow_), または "base64". パラメータが指定されていない場合、デフォルトのエンコーディングは JSON です。

#### 結果:

- `<null>` - トランザクションが見つからないか確認されていない場合
- `<object>` - トランザクションが確認された場合、次のフィールドを持つオブジェクト:
  - `スロット: <u64>` - このトランザクションが処理されたスロット
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `meta: <object | null>` - トランザクションステータスのメタデータオブジェクト:
    - `err: <object | null>` - トランザクションが成功した場合は null。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `手数料: <u64>` - このトランザクションが課金された手数料 u64 integer
    - `preBalances: <array>` - トランザクションが処理される前の u64 アカウント残高の配列
    - `postBalances: <array>` - トランザクションが処理された後の u64 アカウント残高の配列
    - `innerInstructions: <array|undefined>` - このトランザクション中にインナー命令の記録がまだ有効になっていない場合は、 [内側命令の一覧](#inner-instructions-structure) または省略されました
    - `logMessages: <array>` - このトランザクション中にログメッセージの記録がまだ有効になっていない場合は、文字列ログメッセージの配列または省略されました
    - DEPRECATED: `status: <object>` - トランザクションのステータス
      - `"Ok": <null>` - トランザクションが成功しました
      - `"Err": <ERR>` - TransactionError でトランザクションが失敗しました

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

結果：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": {
      "message": {
        "accountKeys": [
          "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
          "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
          "SysvarS1otHashes111111111111111111111111111",
          "SysvarC1ock11111111111111111111111111111111",
          "Vote111111111111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 3,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [1, 2, 3, 0],
            "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
            "programIdIndex": 4
          }
        ],
        "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
      },
      "signatures": [
        "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
      ]
    }
  },
  "id": 1
}
```

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "base64"
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": [
      "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
      "base64"
    ]
  },
  "id": 1
}
```

### getEpochInfo

現在のエポックに関する情報を返します。

#### パラメータ:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果フィールドは、次のフィールドを持つオブジェクトになります。

- `absoluteSlot: <u64>`, the current slot
- `blockHeight: <u64>`, the current block height
- `epoch: <u64>`, the current epoch
- `slotIndex: <u64>`, 現在のエポックの開始時の現在のスロット
- `slotsPerEpoch: <u64>`, 各エポックの最大スロット数

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "absoluteSlot": 166598,
    "blockHeight": 166500,
    "epoch": 27,
    "slotIndex": 2790,
    "slotsInEpoch": 8192
  },
  "id": 1
}
```

### getEpochSchedule

このクラスターのジェネシス設定からエポックスケジュール情報を返します。

#### パラメータ:

該当なし

#### 結果:

結果フィールドは、次のフィールドを持つオブジェクトになります。

- `slotsPerEpoch: <u64>`, 各エポックの最大スロット数
- `leaderScheduleSlotOffset: <u64>`, エポックの開始前のスロットの数
- `ウォームアップ: <bool>`, エポックが短く始まり、成長するかどうか
- `firstNormalEpoch: <u64>`, first normal-length epoch, log2(slotPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH \* (2.pow(firstNormalEpoch) - 1)

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "firstNormalEpoch": 8,
    "firstNormalSlot": 8160,
    "leaderScheduleSlotOffset": 8192,
    "slotsPerEpoch": 8192,
    "warmup": true
  },
  "id": 1
}
```

### getFeeCalculatorForBlockhash

クエリのブロックハッシュに関連付けられた手数料計算機を返します。ブロックハッシュの有効期限が切れている場合は `null` を返します。

#### パラメータ:

- `<string>` - Base58 エンコードされた文字列としてブロックハッシュをクエリします。
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果は `値` 以下に等しい RpcResponse JSON オブジェクトになります:

- `<null>` - クエリブロックハッシュの有効期限が切れている場合
- `<object>` - そうでなければ、以下を含む JSON オブジェクト：
  - `feeCalculator: <object>`, `FeeCalculator` オブジェクト

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getFeeCalculatorForBlockhash",
    "params": [
      "GJxqhuxcgfn5Tcj6y3f8X4FeCDd2RQ6SnEMo1AAxrPRZ"
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 221
    },
    "value": {
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### getFeeRateGoveral

ルートバンクから手数料率ガバナ情報を返します。

#### パラメータ:

該当なし

#### 結果:

`result` フィールドは、次のフィールドを持つ `オブジェクト` になります。

- `burnPercent: <u8>`, 破壊されるために集められた手数料の割合
- `maxLamportsPerSignature: <u64>`, 最大値 `lamportsPerSignature` は次のスロットで達成できます
- `minLamportsPerSignature: <u64>`, 最小値 `lamportsPerSignature` が次のスロットに達成できます
- `targetLamportsPerSignature: <u64>`, クラスタの希望料金率
- `targetSignaturesPerSlot: <u64>`, クラスタの望ましい署名率

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFeeRateGovernor"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": {
      "feeRateGovernor": {
        "burnPercent": 50,
        "maxLamportsPerSignature": 100000,
        "minLamportsPerSignature": 5000,
        "targetLamportsPerSignature": 10000,
        "targetSignaturesPerSlot": 20000
      }
    }
  },
  "id": 1
}
```

### 取得手数料

台帳から最近のブロックハッシュ、それを使ってトランザクションを送信する際のコストを計算するのに使用できる料金表、ブロックハッシュが有効な最後のスロットを返します。

#### パラメータ:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果は、次のフィールドを持つ JSON オブジェクトに `値` が設定された RpcResponse JSON オブジェクトになります。

- `blockhash: <string>` - base-58 エンコードされた文字列としてのハッシュ
- `feeCalculator: <object>` - FeeCalculator オブジェクト、このブロックハッシュの手数料スケジュール。
- `lastValidSlot: <u64>` - ブロックハッシュが有効になる最後のスロット

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      },
      "lastValidSlot": 297
    }
  },
  "id": 1
}
```

### getFirstAvailableBlock

台帳から削除されていない最も低い確認済みブロックのスロットを返します。

#### パラメータ:

該当なし

#### 結果:

- `<u64>` - スロット

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstAvailableBlock"}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": 250000, "id": 1 }
```

### getGenesisHash

ジェネシスハッシュを返します。

#### パラメータ:

該当なし

#### 結果:

- `blockhash: <string>` - base-58 エンコードされた文字列としてのハッシュ

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": "GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC",
  "id": 1
}
```

### getHealth

ノードの現在の健全性を返します。

`solana-validator` に 1 つ以上の `--trusted-validator `引数が指定された場合、ノードが最も高い信頼を得ているバリデータの` HEALTH_CHECK_SLOT_DISTANCE` スロット内にある場合は "ok" が返され、そうでない場合はエラーが返されます。 信頼できるバリデータが提供されていない場合は、常に "ok" が返されます。

#### パラメータ:

該当なし

#### 結果:

ノードが健全な場合: "ok" ノードが不健全な場合、JSON RPC エラー応答が返されます。 エラーレスポンスの仕様は**UNSTABLE**であり、将来的に変更される可能性があります。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

健全な結果:

```json
{ "jsonrpc": "2.0", "result": "ok", "id": 1 }
```

不健全な結果 (一般的):

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is unhealthy",
    "data": {}
  },
  "id": 1
}
```

不健全な結果 (追加情報がある場合)

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is behind by 42 slots",
    "data": {
      "numSlotsBehind": 42
    }
  },
  "id": 1
}
```

### getIdentity

現在のノードの Id pubkey を返します

#### パラメータ:

該当なし

#### 結果:

結果フィールドは、次のフィールドを持つ JSON オブジェクトになります。

- `identity`, 現在のノード \(Base-58 エンコードされた文字列\) の Id pubkey

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": { "identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4W2DJBppN" },
  "id": 1
}
```

### getInflationGoveral

現在のインフレーションガバナを返します

#### パラメータ:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果フィールドは、次のフィールドを持つ JSON オブジェクトになります。

- `initial: <f64>`, 時間 0 からの初期インフレ率
- `ターミナル: <f64>`, ターミナルインフレ率
- `テーパー: <f64>`, インフレ率が引き下げられる年
- `財団: <f64>`, 財団に割り当てられた総インフレ率
- `財団期間: <f64>`, 年間の財団プールインフレの持続時間

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "foundation": 0.05,
    "foundationTerm": 7,
    "initial": 0.15,
    "taper": 0.15,
    "terminal": 0.015
  },
  "id": 1
}
```

### getInflationRate

現在のエポックの特定のインフレ値を返します。

#### パラメータ:

該当なし

#### 結果:

結果フィールドは、次のフィールドを持つ JSON オブジェクトになります。

- `total: <f64>`, total inflation
- `バリデータ: <f64>`, バリデータに割り当てられたインフレーション。
- `財団: <f64>`, 財団に割り当てられたインフレーション。
- `エポック: <f64>`, これらの値が有効なエポック

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "epoch": 100,
    "foundation": 0.001,
    "total": 0.149,
    "validator": 0.148
  },
  "id": 1
}
```

### getLargestAccounts

ランポート残高で最大 20 のアカウントを返します

#### パラメータ:

- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - (任意) `filter: <string>` - 口座の種類で結果をフィルターします。現在サポートされています: `循環|循環させない`

#### 結果:

結果は配列の `値` に等しい RpcResponse JSON オブジェクトになります:

- `<object>` - そうでなければ、以下を含む JSON オブジェクト：
  - `アドレス: <string>`, アカウントの base-58 エンコードされたアドレス
  - `lamports: <u64>`, u64 としてのアカウント内のランポート数。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": [
      {
        "lamports": 999974,
        "address": "99P8ZgtJYe1buSK8JXkvpLh8xPsCFuLYhz9hQFNw93WJ"
      },
      {
        "lamports": 42,
        "address": "uPwWLo16MVehpyWqsLkK3Ka8nLowWvAHbBChqv2FZeL"
      },
      {
        "lamports": 42,
        "address": "aYJCgU7REfu3XF8b3QhkqgqQvLizx8zxuLBHA25PzDS"
      },
      {
        "lamports": 42,
        "address": "CTvHVtQ4gd4gUcw3bdVgZJJqApXE9nCbbbP4VTS5wE1D"
      },
      {
        "lamports": 20,
        "address": "4fq3xJ6kfrh9RkJQsmVd5gNMvJbuSHfErywvEjNQDPxu"
      },
      {
        "lamports": 4,
        "address": "AXJADheGVp9cruP8WYu46oNkRbeASngN5fPCMVGQqNHa"
      },
      {
        "lamports": 2,
        "address": "8NT8yS6LiwNprgW4yM1jPPow7CwRUotddBVkrkWgYp24"
      },
      {
        "lamports": 1,
        "address": "SysvarEpochSchedu1e111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Stake11111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarC1ock11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "StakeConfig11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRent111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Config1111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarStakeHistory1111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRecentB1ockHashes11111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarFees111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Vote111111111111111111111111111111111111111"
      }
    ]
  },
  "id": 1
}
```

### getLeaderSchedule

エポックのリーダースケジュールを返します

#### パラメータ:

- `<u64>` - (任意) 与えられたスロットに対応するエポックのリーダースケジュールを取得します。 指定されていない場合、現在のエポックのリーダースケジュールが取得されます。
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `<null>` - 要求されたエポックが見つからない場合
- `<object>` - そうでない場合は、リーダーの公開キーの辞書\(Base-58 でエンコードされた文字列\)と、それに対応するリーダーのスロットのインデックスの値(インデックスは、リクエストされたエポックの最初のスロットからの相対値)となります。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F": [
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
      21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
      39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
      57, 58, 59, 60, 61, 62, 63
    ]
  },
  "id": 1
}
```

### getMinimumBalanceForRentExemption

アカウント賃貸料免除を行うために必要な最小残高を返します。

#### パラメータ:

- `<usize>` - アカウントデータの長さ
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `<u64>` - アカウントに必要な最小ランポート数

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

### getMultipleAccounts

公開キーのリストのアカウント情報を返します。

#### パラメータ:

- `<array>` -base-58 文字列でエンコードされたクエリをするためのパブキーの配列。
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "base58" は 128 バイト未満のアカウントデータに制限されています。 "base64" は base64 エンコードされたデータを任意のサイズの Account データに対して返します。 "base64+zstd" は、 [Zstandard](https://facebook.github.io/zstd/) を使用してアカウントデータを圧縮し、結果を base64 エンコードします。 "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されていてパーサが見つからない場合、フィールドは "base64" エンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。
  - (オプション) `dataSlice: <object>` - 指定された `オフセット: <usize>` と `長さ: <usize>` フィールド; "base58", "base64" または "base64+zstd" エンコーディングでのみ使用できます。

#### 結果:

結果は `値` 以下に等しい RpcResponse JSON オブジェクトになります:

以下の配列:

- `<null>` - その Pubkey のアカウントが存在しない場合
- `<object>` - そうでなければ、以下を含む JSON オブジェクト：
  - `lamports: <u64>`, u64 としてこのアカウントに割り当てられたラムポートの数。
  - `owner: <string>`, このアカウントに割り当てられたプログラムの base-58 でエンコードされたパブキー
  - `data:<文字列、エンコーディング>`]、アカウントに関連するデータ、エンコーディングされたバイナリデータまたはエンコード依存の JSON フォーマット `{<program>: <state>}`
  - `executable: <bool>`, アカウントにプログラム \(および厳密には読み取り専用\) が含まれているかを示す boolean
  - `rentEpoch: <u64>`, u64 としてこのアカウントが賃貸借義務を負うエポック

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "dataSlice": {
          "offset": 0,
          "length": 0
        }
      }
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": ["AAAAAAEAAAACtzNsyJrW0g==", "base64"],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base64"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "encoding": "base58"
      }
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
          "base58"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base58"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

### getProgramAccounts

提供されたプログラムパブキーが所有するすべてのアカウントを返します。

#### パラメータ:

- `<string>` -base-58 文字列としてエンコードされたプログラムのパブキー
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "base58" は 128 バイト未満のアカウントデータに制限されています。 "base64" は base64 エンコードされたデータを任意のサイズの Account データに対して返します。 "base64+zstd" は、 [Zstandard](https://facebook.github.io/zstd/) を使用してアカウントデータを圧縮し、結果を base64 エンコードします。 "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されていてパーサが見つからない場合、フィールドは "base64" エンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。
  - (オプション) `dataSlice: <object>` - 指定された `オフセット: <usize>` と `長さ: <usize>` フィールド; "base58", "base64" または "base64+zstd" エンコーディングでのみ使用できます。
  - (任意) `filters: <array>` - さまざまな [フィルター オブジェクト](jsonrpc-api.md#filters)を使用したフィルター結果。 アカウントは、結果に含めるすべてのフィルタ条件を満たしている必要があります

##### フィルター:

- `memcmp: <object>` - 特定のオフセット時に提供される一連のバイトとプログラムアカウントデータを比較する。 フィールド:

  - `offset: <usize>` - 比較を開始するためのプログラムアカウントデータのオフセット
  - `bytes: <string>` - 一致するデータ base-58 エンコードされた文字列

- `dataSize: <u64>` - プログラムアカウントのデータ長と提供されるデータサイズを比較します。

#### 結果:

Result フィールドは JSON オブジェクトの配列になり、以下のようになります。

- `pubkey: <string>` - Base-58 でエンコードされた文字列としてのアカウントパブキー
- `account: <object>` - 次のサブフィールドを持つ JSON オブジェクト:
  - `lamports: <u64>`, u64 としてこのアカウントに割り当てられたランポートの数。
  - `owner:<string>`このアカウントが割り当てられているプログラムの Base-58 でエンコードされたパブキー。 `data: <[string,encoding]|object>`, アカウントに関連するデータで、エンコードされたバイナリデータまたはエンコードパラメータ依存の JSON 形式 `{<program>: <state>}`。
  - `executable: <bool>`, アカウントにプログラム \(および厳密には読み取り専用\) が含まれているかを示す boolean
  - `rentEpoch: <u64>`, u64 としてこの口座が賃貸借義務を負うエポック。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getProgramAccounts",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "filters": [
          {
            "dataSize": 17
          },
          {
            "memcmp": {
              "offset": 4,
              "bytes": "3Mc6vR"
            }
          }
        ]
      }
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

### getRecentBlockhash

台帳から最近のブロックハッシュを返します。 そして手数料のスケジュールを使って取引の提出コストを計算することができます

#### パラメータ:

- `<object>` - (任意) [コミットメント](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

文字列ブロックハッシュと"FeeCalculator JSON"オブジェクトで構成される"JSON オブジェクト"を含む RpcResponse

- `RpcResponse<object>` - JSON オブジェクトを含む `値` フィールドを持つ RpcResponse JSON オブジェクト
- `blockhash: <string>` - base-58 でエンコードされた文字列としてのハッシュ
- `feeCalculator: <object>` - FeeCalculator オブジェクト、このブロックハッシュの手数料スケジュール。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d 'i
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### getRecentPerformanceSamples

最近のパフォーマンスサンプルのリストを逆スロット順に返します。 パフォーマンスサンプルは 60 秒ごとに取得され、特定の時間枠で発生したトランザクション数とスロット数が含まれます。

#### パラメータ:

- `limit: <usize>` - (任意)返却するサンプル数(最大 720)

#### 結果:

以下の配列:

- `RpcPerfSample<object>`
  - `スロット: <u64>` - サンプルを採取したスロット
  - `numTransactions: <u64>` - サンプル内のトランザクション数
  - `numSlots: <u64>` - サンプル内のスロット数
  - `samplePeriod Secs: <u16>` - サンプルウィンドウ内の秒数

#### 例:

リクエスト:

```bash
// Request
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getRecentPerformanceSamples", "params": [4]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 348125
    },
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 347999
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347873
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347748
    }
  ],
  "id": 1
}
```

### getSnapshotSlot

ノードがスナップショットを持つ最高のスロットを返します。

#### パラメータ:

該当なし

#### 結果:

- `<u64>` - Snapshot slot

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSnapshotSlot"}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": 100, "id": 1 }
```

ノードにスナップショットがない場合の結果:

```json
{
  "jsonrpc": "2.0",
  "error": { "code": -32008, "message": "No snapshot" },
  "id": 1
}
```

### getSignatureStatuses

署名のリストのステータスを返します。 Unless the `searchTransactionHistory` configuration parameter is included, this method only searches the recent status cache of signatures, which retains statuses for all active slots plus `MAX_RECENT_BLOCKHASHES` rooted slots.

#### パラメータ:

- `<array>` - base-58 エンコードされた文字列として確認するためのトランザクション署名の配列
- `<object>` - (任意) 次のフィールドを含む設定オブジェクト:
  - `searchTransactionHistory: <bool>` - true の場合は、Solana ノードは最近のステータスキャッシュにない署名を台帳キャッシュで検索します。

#### 結果:

TransactionStatus オブジェクトの配列で構成された JSON オブジェクトを含む RpcResponse です。

- `RpcResponse<object>` - `value` フィールドを持つ RpcResponse JSON オブジェクト:

以下の配列:

- `<null>` - 不明な取引
- `<object>`
  - `slot: <u64>` - トランザクションが処理されたスロット
  - `confirmations: <usize | null>` - クラスタの過半数によって確定された署名確認からのブロック数。
  - `err: <object | null>` - トランザクションが成功した場合は null。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus: <string | null>` - トランザクションのクラスタ確認ステータス; `processed`, `confirmed`, `finalized`. 楽観的な確認については、 [コミット](jsonrpc-api.md#configuring-state-commitment) を参照してください。
  - DEPRECATED: `status: <object>` - トランザクションのステータス
    - `"Ok": <null>` - トランザクションが成功しました
    - `"Err": <ERR>` - TransactionError でトランザクションが失敗しました

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
        "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"
      ]
    ]
  }
'
```

結果:

```json
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
        },
        "confirmationStatus": "confirmed"
      },
      null
    ]
  },
  "id": 1
}
```

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"
      ],
      {
        "searchTransactionHistory": true
      }
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        },
        "confirmationStatus": "finalized"
      },
      null
    ]
  },
  "id": 1
}
```

### getSlot

ノードが処理しているスロットを返します。

#### パラメータ:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `<u64>` - 現在のスロット

#### 例:

リクエスト：

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

リクエスト：

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getSlotLeader

現在のスロットリーダを返します

#### パラメータ:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `<string>` - base-58 文字列でエンコードされたアカウントアドレス

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

結果：

```json
{
  "jsonrpc": "2.0",
  "result": "ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS",
  "id": 1
}
```

### getStakeActivation

ステーキングアカウントのエポック有効化情報を返します。

#### パラメータ:

- `<string>` - base-58 文字列でエンコードされたクエリをするためのトークンアカウントのパブキー
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - (任意) `エポック: <u64>` - アクティベーションの詳細を計算するエポック。 パラメータが指定されていない場合、デフォルトは現在のエポックです。

#### 結果:

結果は以下のフィールドを持つ JSON オブジェクトになります。

- `state: <string` -ステーキングアカウントの起動状態（`active`、`inactive`、`activating`、`deactivating`のいずれか）
- `active: <u64>` - エポック時にアクティブにステーキングします
- `inactive: <u64>` - エポック時に非アクティブになります

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

結果：

```json
{
  "jsonrpc": "2.0",
  "result": { "active": 197717120, "inactive": 0, "state": "active" },
  "id": 1
}
```

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getStakeActivation",
    "params": [
      "CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT",
      {
        "epoch": 4
      }
    ]
  }
'
```

結果：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "active": 124429280,
    "inactive": 73287840,
    "state": "activating"
  },
  "id": 1
}
```

### getSupply

現在の供給に関する情報を返します。

#### パラメータ:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果は配列の `値` に等しい RpcResponse JSON オブジェクトになります:

- `total: <u64>` - ランポートの供給量
- `循環: <u64>` - ランポートでの循環供給。
- `非循環: <u64>` - ランポートの非循環供給
- `非循環勘定科目: <array>` - 循環していないアカウントのアカウントアドレスを文字列として配列します。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getSupply"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "circulating": 16000,
      "nonCirculating": 1000000,
      "nonCirculatingAccounts": [
        "FEy8pTbP5fEoqMV1GdTz83byuA8EKByqYat1PKDgVAq5",
        "9huDUZfxoJ7wGMTffUE7vh1xePqef7gyrLJu9NApncqA",
        "3mi1GmwEE3zo2jmfDuzvjSX9ovRXsDUKHvsntpkhuLJ9",
        "BYxEJTDerkaRWBem3XgnVcdhppktBXa2HbkHPKj2Ui4Z"
      ],
      "total": 1016000
    }
  },
  "id": 1
}
```

### getTokenAccountBalance

SPL トークンアカウントのトークン残高を返します。 **UNSTABLE**

#### パラメータ:

- `<string>` - base-58 文字列でエンコードされたクエリするためのトークンアカウントのパブキー
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果は配列の `値` に等しい RpcResponse JSON オブジェクトになります:

- `uiAmount: <f64>` - ミント所定の小数を使用してバランスをとる。
- `amount: <string>` - u64 の小数点以下の生の残高。
- `decimals: <u8>` - 小数点以下の桁数。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStuGbc3sM84cKR6yYaaSstyLSU4ve5oovLS7"]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 98.64,
      "amount": "9864",
      "decimals": 2
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

承認されたデリゲートによってすべての SPL トークンアカウントを返します。 **UNSTABLE**

#### パラメータ:

- `<string>` - base-58 文字列でエンコードされた、クエリに委任するアカウントのパブキー
- `<object>` - 以下のいずれか:
  - `mint: <string>` - アカウントを base-58 エンコードされた文字列として制限する特定のトークンの公開キー
  - `programId: <string>` - Base-58 エンコードされた文字列として、アカウントを所有するトークンプログラム ID の公開キー
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されているが、特定のアカウントで有効な Mint が見つからない場合、そのアカウントは結果から除外されます。
  - (オプション) `dataSlice: <object>` - 指定された `オフセット: <usize>` と `長さ: <usize>` フィールド; "base58", "base64" または "base64+zstd" エンコーディングでのみ使用できます。

#### 結果:

結果は `値` を持つ RpcResponse JSON オブジェクトになり、以下に含まれる JSON オブジェクトの配列と同じになります。

- `pubkey: <string>` - Base-58 でエンコードされた文字列としてのアカウント公開キー
- `account: <object>` - 次のサブフィールドを持つ JSON オブジェクト:
  - `lamports: <u64>`, u64 でこのアカウントに割り当てられたランポートの数。
  - `owner: <string>`, このアカウントに割り当てられた base-58 でエンコードされたプログラムの公開キー
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, アカウントにプログラム \(および厳密には読み取り専用\) が含まれているかを示す boolean
  - `rentEpoch: <u64>`, u64 でこの口座が賃貸借義務を負うエポック。

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByDelegate",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "CnPoSPKXu7wJqxe59Fs72tkBeALovhsCxYeFwPCQH9TD"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### getTokenAccountsByOwner

トークン所有者によってすべての SPL トークンアカウントを返します。 **UNSTABLE**

#### パラメータ:

- `<string>` -base-58 文字列でエンコードされたクエリする為のアカウント所有者の公開キー
- `<object>` - 以下のいずれか:
  - `mint: <string>` - アカウントを base-58 エンコードされた文字列として制限する特定のトークンの公開キー
  - `programId: <string>` - Base-58 エンコードされた文字列として、アカウントを所有するトークンプログラム ID の公開キー
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されていますが、特定のアカウントで有効な Mint が見つからない場合、そのアカウントは結果から除外されます。
  - (オプション) `dataSlice: <object>` - 指定された `オフセット: <usize>` と `長さ: <usize>` フィールド; "base58", "base64" または "base64+zstd" エンコーディングでのみ使用できます。

#### 結果:

結果は `値` を持つ RpcResponse JSON オブジェクトになり、以下に含まれる JSON オブジェクトの配列と同じになります。

- `pubkey: <string>` - Base-58 エンコードされた文字列としてのアカウント公開キー
- `account: <object>` - 次のサブフィールドを持つ JSON オブジェクト:
  - `lamports: <u64>`, このアカウントに u64 として割り当てられたランポートの数。
  - `owner: <string>`, このアカウントに割り当てられたプログラムの base-58 でエンコードされた公開キー
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, アカウントにプログラム \(および厳密には読み取り専用\) が含まれているかを示すブーレン
  - `rentEpoch: <u64>`, この口座が u64 として賃貸借義務を負うエポック。

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByOwner",
    "params": [
      "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F",
      {
        "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

結果：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": null,
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### getTokenLargestAccounts

特定の SPL トークンタイプの 20 の最大アカウントを返します。 **UNSTABLE**

#### パラメータ:

- `<string>` -base-58 文字列でエンコードされたクエリするトークン Mint の公開キー
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果は `値` を含む JSON オブジェクトの配列と同じ値を持つ RpcResponse JSON オブジェクトになります。

- `アドレス: <string>`, トークンアカウントアドレス
- `uiAmount: <f64>` - ミント処方の小数点以下を使用するトークンの総供給量。
- `amount: <string>` - u64 の文字列表現での小数点以下の生の残高。
- `decimals: <u8>` - 小数点以下の桁数。

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "address": "FYjHNoFtSQ5uijKrZFyYAxvEr87hsKXkXcxkcmkBAf4r",
        "amount": "771",
        "decimals": 2,
        "uiAmount": 7.71
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29
      }
    ]
  },
  "id": 1
}
```

### getTokenSupply

SPL トークンタイプの総供給量を返します。 **UNSTABLE**

#### パラメータ:

- `<string>` -base-58 文字列でエンコードされたクエリするトークン Mint の 公開キー
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果は配列の `値` に等しい RpcResponse JSON オブジェクトになります:

- `uiAmount: <f64>` - トークンの総供給量、ミント処方の小数点以下を使用する。
- `amount: <string>` - u64 文字列表現の小数点以下の生トークンの総供給量。
- `decimals: <u8>` - 小数点以下の桁数。

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 1000,
      "amount": "100000",
      "decimals": 2
    }
  },
  "id": 1
}
```

### getTransactionCount

台帳から現在の取引数を返します。

#### パラメータ:

- `<object>` - (任意) [コミットメント](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `<u64>` - カウント

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1,"method":"getTransactionCount"}
'

```

結果:

```json
{ "jsonrpc": "2.0", "result": 268, "id": 1 }
```

### getVersion

ノード上で実行されている現在の Solana のバージョンを返します。

#### パラメータ:

該当なし

#### 結果:

結果フィールドは、次のフィールドを持つ JSON オブジェクトになります。

- `solana-core`, solana-core のソフトウェアバージョン
- `feature-set`, 現在のソフトウェアの機能セットの固有の識別子

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": { "solana-core": "1.6.0" }, "id": 1 }
```

### getVoteAccounts

現在の銀行のすべての投票口座の口座情報と関連するステーキングを返します。

#### パラメータ:

- `<object>` - (任意) [コミットメント](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

結果フィールドは、 `現在の` と `delinquent` アカウントの JSON オブジェクトになります。 それぞれに、次のサブフィールドを持つ JSON オブジェクトの配列が含まれています。

- `votePubkey: <string>` - Base-58 文字列でエンコードされた投票アカウント公開キー
- `nodePubkey: <string>` -Base-58 文字列でエンコードされたノード公開キー
- `activatedStake: <u64>` - この投票アカウントに委任され、この時期にアクティブになります。
- `epochVoteAccount: <bool>` - bool, 投票アカウントがこのエポックにステーキングされているかどうか
- `コミッション: <number>`, 投票口座に支払う報酬の割合 (0-100)
- `lastVote: <u64>` - 最近のスロットがこの投票アカウントで投票しました。
- `epochCredits: <array>` - 各エポックの終わりまでに獲得したクレジット数の履歴を、以下を含む配列の配列として表します。`[epoch, credits, previousCredits]` - 各エポックが終了するまでに獲得したクレジット数の履歴。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "current": [
      {
        "commission": 0,
        "epochVoteAccount": true,
        "epochCredits": [
          [1, 64, 0],
          [2, 192, 64]
        ],
        "nodePubkey": "B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD",
        "lastVote": 147,
        "activatedStake": 42,
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ],
    "delinquent": [
      {
        "commission": 127,
        "epochVoteAccount": false,
        "epochCredits": [],
        "nodePubkey": "6ZPxeQaDo4bkZLRsdNrCzchNQr5LN9QMc9sipXv9Kw8f",
        "lastVote": 0,
        "activatedStake": 0,
        "votePubkey": "CmgCk4aMS7KW1SHX3s9K5tBJ6Yng2LBaC8MFov4wx9sm"
      }
    ]
  },
  "id": 1
}
```

### minimumLedgerSlot

台帳にノードの情報を含む一番下のスロットを返します。 この値は、ノードが古い台帳データをパージするように設定されている場合、時間の経過とともに増加することがあります。

#### パラメータ:

該当なし

#### 結果:

- `u64` - 最小台帳スロット

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1,"method":"minimumLedgerSlot"}
'

```

結果:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### requestAirdrop

公開キーにランポートのエアドロップを要求します。

#### パラメータ:

- `<string>` -base-58 文字列でエンコードされたランポートを受け取るためのアカウント公開キー。
- `<integer>` -u64 としてのランポート
- `<object>` - (任意) [コミットメント](jsonrpc-api.md#configuring-state-commitment) (ブロックハッシュの取得とエアドロップの成功の確認に使用)

#### 結果:

- `<string>` - Base-58 文字列でエンコードされたエアドロップのトランザクション署名。

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}
'

```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
  "id": 1
}
```

### トランザクションを送信しよう。

署名済みトランザクションをクラスタに送信します。

このメソッドはいかなる方法でもトランザクションを変更することはありません; クライアントによって作成されたトランザクションをそのままノードにリレーします。

ノードの rpc サービスがトランザクションを受信した場合、このメソッドは直ちに 確認を待たずに成功します。 このメソッドからの応答が成功しても、そのトランザクションがクラスターで処理または確認されることは保証されません。

Rpc サービスはそれを提出することを合理的に再試行しますが、トランザクションの が `recent_blockhash` の有効期限が切れると、トランザクションが拒否される可能性があります。

[`getSignatureStatuss`](jsonrpc-api.md#getsignaturestatuses) を使用して、 トランザクションが処理され確認されたことを確認します。

提出する前に、次のプリフライトチェックが実行されます:

1. トランザクション署名が確認されました。
2. このトランザクションは、"preflight"コミットメントによって指定された銀行スロットに対してシミュレートされます。 失敗した場合はエラーが返されます。 必要に応じてプリフライトチェックは無効になっている可能性があります。 混乱する振る舞いを避けるために、同じコミットメントと事前コミットメントを指定することをお勧めします。

返される署名はトランザクション内の最初の署名であり、トランザクションを識別するために使用されます([トランザクション ID](../../terminology.md#transanction-id))。 この識別子は、提出前にトランザクションデータから簡単に抽出できます。

#### パラメータ:

- `<string>` - 符号化された文字列
- `<object>` - (任意) 次のフィールドを含む設定オブジェクト:
  - `skipPreflight: <bool>` - true の場合、プリフライトトランザクションチェックをスキップします (デフォルト: false)
  - `preflightCommitment: <string>` - (任意) [プリフライトに使用する](jsonrpc-api.md#configuring-state-commitment) レベル (デフォルト: `"max"`).
  - `エンコーディング: <string>` - (任意) トランザクションデータに使用されるエンコーディング。 `"base58"` (_slow_, **DEPRECATED**), または `"base64"`. (デフォルト: `"base58"`).

#### 結果:

- `<string>` - トランザクションに埋め込まれた First Transaction Signature as base-58 encoded string ([transaction id](../../terminology.md#transanction-id))

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sendTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'

```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": "2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwe5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb",
  "id": 1
}
```

### simulateTransaction

トランザクションの送信をシミュレートする

#### パラメータ:

- `<string>` - エンコードされた文字列としてのトランザクション。 トランザクションには有効な blockhh が必要ですが、署名する必要はありません。
- `<object>` - (任意) 次のフィールドを含む設定オブジェクト:
  - `sigVerify: <bool>` - true の場合、トランザクション署名が検証されます (デフォルト: false)
  - `commitment: <string>` - (任意) [トランザクションをシミュレートする](jsonrpc-api.md#configuring-state-commitment) レベル (デフォルト: `"max"`) 。
  - `エンコーディング: <string>` - (任意) トランザクションデータに使用されるエンコーディング。 `"base58"` (_slow_, **DEPRECATED**), または `"base64"`. (デフォルト: `"base58"`).

#### 結果:

TransactionStatus オブジェクトを含む RpcResponse 結果は次のフィールドを持つ `値` JSON オブジェクトに設定された RpcResponse JSON オブジェクトになります。

- `err: <object | string | null>` - トランザクションが成功した場合は null。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - 実行中にトランザクション命令が出力されるログメッセージの配列。 トランザクションが実行できる前にシミュレーションが失敗した場合、null (例えば、無効なブロックハッシュや署名の検証に失敗した場合など)

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "simulateTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'
```

結果：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 218
    },
    "value": {
      "err": null,
      "logs": [
        "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
      ]
    }
  },
  "id": 1
}
```

### setLogFilter

バリデータにログフィルタを設定します。

#### パラメータ:

- `<string>` - 使用する新しいログフィルタ

#### 結果:

- `<null>`

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"setLogFilter", "params":["solana_core=debug"]}
'
```

結果：

```json
{ "jsonrpc": "2.0", "result": null, "id": 1 }
```

### validatorExit

RPC 終了を有効にしてバリデータを起動した場合 (`--enable-rpc-exit` パラメータ)、このリクエストはバリデータを終了させます。

#### パラメータ:

該当なし

#### 結果:

- `<bool>` - バリデータの終了に成功したかどうか

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"validatorExit"}
'

```

結果：

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

## サブスクリプション Web ソケット

`ws://<<ADDRESS>/`の RPC PubSub Websocket に接続した後。

- 以下の方法でサブスクリプション要求をウェブソケットに送信します。
- 複数のサブスクリプションが同時にアクティブになることがあります。
- 多くのサブスクリプションでは、オプションの [`コミットメント` パラメータ](jsonrpc-api.md#configuring-state-commitment)を取得し、通知をトリガーする変更がどのように最終化されるべきかを定義します。 サブスクリプションの場合、コミットメントが指定されていない場合、デフォルト値は` "singleGossip"`です。

### accountSubscribe

アカウントに登録すると、指定したアカウントの公開キーのランポートやデータが変更されたときに通知を受け取ることができます。

#### パラメータ:

- `<string>` - base-58 文字列でエンコードされたアカウントアドレス
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - `<object>` - (任意) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd" または "jsonParsed". "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" がリクエストされてもパーサが見つからない場合、フィールドはバイナリエンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。

#### 結果:

- `<number>` - サブスクリプション ID \(購読解除に必要)

#### 例:

リクエスト:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "root"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "jsonParsed"
    }
  ]
}
```

結果:

```json
{ "jsonrpc": "2.0", "result": 23784, "id": 1 }
```

#### 通知フォーマット:

Base58 エンコーディング:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
          "base58"
        ],
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

パース-JSON エンコーディング:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": {
          "program": "nonce",
          "parsed": {
            "type": "initialized",
            "info": {
              "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
              "feeCalculator": {
                "lamportsPerSignature": 5000
              }
            }
          }
        },
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

### アカウント登録解除

アカウント変更通知の購読を解除する。

#### パラメータ:

- `<number>` - キャンセルするアカウントサブスクリプション ID

#### 結果:

- `<bool>` - 登録解除成功メッセージ

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "accountUnsubscribe", "params": [0] }
```

結果:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### logsSubscribe

取引ログを購読します。 **UNSTABLE**

#### パラメータ:

- `filter: <string>|<object>` - 口座タイプ別に結果を受け取るためのログの条件をフィルタリングします。現在サポートされているものは以下です。
  - "all" - シンプルな投票トランザクションを除くすべてのトランザクションを購読します。
  - "allWithVotes" - シンプルな投票トランザクションを含むすべてのトランザクションを購読します。
  - `{ "mentions": [ <string> ] }` - 提供された Pubkey を記載するすべてのトランザクションを購読 (base-58 エンコードされた文字列として)
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `<integer>` - サブスクリプション ID \(購読解除に必要)

#### 例:

リクエスト:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [
    {
      "mentions": [ "11111111111111111111111111111111" ]
    },
    {
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [ "all" ]
}
```

結果:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### 通知フォーマット:

Base58 エンコーディング:

```json
{
  "jsonrpc": "2.0",
  "method": "logsNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
        "err": null,
        "logs": [
          "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
        ]
      }
    },
    "subscription": 24040
  }
}
```

### logsUnsubscribe

トランザクションログを購読解除します。

#### パラメータ:

- `<integer>` - キャンセルするサブスクリプション ID

#### 結果:

- `<bool>` - 登録解除成功メッセージ

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "logsUnsubscribe", "params": [0] }
```

結果:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### programSubscribe

プログラムが変更されたときに通知を受け取るためにプログラムを購読します。

#### パラメータ:

- `<string>` - Base-58 文字列でエンコードされたプログラム ID の公開キー
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd" または "jsonParsed". "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" がリクエストされていてパーサが見つからない場合、フィールドは base64 エンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。
  - (任意) `filters: <array>` - さまざまな [フィルター オブジェクト](jsonrpc-api.md#filters)を使用したフィルター結果。 アカウントは、結果に含めるすべてのフィルタ条件を満たしている必要があります

#### 結果:

- `<integer>` - サブスクリプション ID \(購読解除に必要)

#### 例:

リクエスト:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "jsonParsed"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "filters": [
        {
          "dataSize": 80
        }
      ]
    }
  ]
}
```

結果:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### 通知フォーマット:

Base58 エンコーディング:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": [
            "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
            "base58"
          ],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

パース-JSON エンコーディング:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": {
            "program": "nonce",
            "parsed": {
              "type": "initialized",
              "info": {
                "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
                "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
                "feeCalculator": {
                  "lamportsPerSignature": 5000
                }
              }
            }
          },
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

### programUnsubscribe

アカウント変更通知の購読を解除します。

#### パラメータ:

- `<integer>` - キャンセルするアカウントサブスクリプション ID

#### 結果:

- `<bool>` - 登録解除成功メッセージ

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programUnsubscribe", "params": [0] }
```

結果：

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### signatureSubscribe

トランザクションの署名を購読し、トランザクションが確認されたときに通知を受け取る`signatureNotification`では、購読が自動的にキャンセルされます。

#### パラメータ:

- `<string>` - Base-58 エンコードされた文字列としてのトランザクション署名
- `<object>` - (任意) [コミットメント](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

- `integer` - サブスクリプション ID \(購読解除に必要\)

#### 例:

リクエスト:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b"
  ]
}

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b",
    {
      "commitment": "max"
    }
  ]
}
```

結果:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### 通知フォーマット:

```bash
{
  "jsonrpc": "2.0",
  "method": "signatureNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5207624
      },
      "value": {
        "err": null
      }
    },
    "subscription": 24006
  }
}
```

### signatureUnsubscribe

署名確認通知を購読解除します。

#### パラメータ:

- `<integer>` - キャンセルするサブスクリプション ID

#### 結果:

- `<bool>` - 登録解除成功メッセージ

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "signatureUnsubscribe", "params": [0] }
```

結果:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### slotSubscribe

スロットがバリデータによって処理されるたびに通知を受け取るように登録します。

#### パラメータ:

該当なし

#### 結果:

- `integer` - サブスクリプション ID \(購読解除に必要\)

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }
```

結果:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### 通知フォーマット:

```bash
{
  "jsonrpc": "2.0",
  "method": "slotNotification",
  "params": {
    "result": {
      "parent": 75,
      "root": 44,
      "slot": 76
    },
    "subscription": 0
  }
}
```

### slotUnsubscribe

スロット通知の購読を解除します。

#### パラメータ:

- `<integer>` - キャンセルするサブスクリプション ID

#### 結果:

- `<bool>` - 登録解除成功メッセージ

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotUnsubscribe", "params": [0] }
```

結果:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### rootSubscribe

新しいルートがバリデータによって設定されるたびに通知を受け取るように購読します。

#### パラメータ:

該当なし

#### 結果:

- `integer` - subscription id \(needed to unsubscribe\)

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootSubscribe" }
```

結果:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### 通知フォーマット:

結果は最新のルートスロット番号です。

```bash
{
  "jsonrpc": "2.0",
  "method": "rootNotification",
  "params": {
    "result": 42,
    "subscription": 0
  }
}
```

### ルート購読解除

ルート通知を購読解除する

#### パラメータ:

- `<integer>` - キャンセルするサブスクリプション ID

#### 結果:

- `<bool>` - 登録解除成功メッセージ

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootUnsubscribe", "params": [0] }
```

結果:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### voteSubscribe -デフォルトで無効になっています。

**このサブスクリプションは不安定で、バリデータが `--rpc-pubs-enable-vote-subscription` フラグで開始された場合にのみ利用できます。 このサブスクリプションのフォーマットは将来変更される可能性があります**

ゴシップで新しい投票が観察されたらいつでも通知を受け取るようにします。 これらの投票は、したがって、これらの投票が元帳に入る保証はありません。

#### パラメータ:

該当なし

#### 結果:

- `integer` - subscription id \(needed to unsubscribe\)

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteSubscribe" }
```

結果:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### 通知フォーマット:

結果は最新の投票で、ハッシュ、投票済みスロットのリスト、オプションのタイムスタンプが含まれます。

```json
{
  "jsonrpc": "2.0",
  "method": "voteNotification",
  "params": {
    "result": {
      "hash": "8Rshv2oMkPu5E4opXTRyuyBeZBqQ4S477VG26wUTFxUM",
      "slots": [1, 2],
      "timestamp": null
    },
    "subscription": 0
  }
}
```

### 投票購読解除

スロット通知の購読を解除します。

#### パラメータ:

- `<integer>` - キャンセルするサブスクリプション ID

#### 結果:

- `<bool>` - 登録解除成功メッセージ

#### 例:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteUnsubscribe", "params": [0] }
```

応答:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```
