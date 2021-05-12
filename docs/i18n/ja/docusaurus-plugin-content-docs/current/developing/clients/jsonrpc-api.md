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
- [getBlock](jsonrpc-api.md#getblock)
- [getBlockProduction](jsonrpc-api.md#getblockproduction)
- [getBlockCommitment](jsonrpc-api.md#getblockcommitment)
- [getBlocks](jsonrpc-api.md#getblocks)
- [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit)
- [getBlockTime](jsonrpc-api.md#getblocktime)
- [getClusterNodes](jsonrpc-api.md#getclusternodes)
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
- [getInflationReward](jsonrpc-api.md#getinflationreward)
- [getLargestAccounts](jsonrpc-api.md#getlargestaccounts)
- [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
- [getMaxRetransmitSlot](jsonrpc-api.md#getmaxretransmitslot)
- [getMaxShredInsertSlot](jsonrpc-api.md#getmaxshredinsertslot)
- [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [getMultipleAccounts](jsonrpc-api.md#getmultipleaccounts)
- [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
- [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
- [getRecentPerformanceSamples](jsonrpc-api.md#getrecentperformancesamples)
- [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress)
- [getSignatureStatuses](jsonrpc-api.md#getsignaturestatuses)
- [getSlot](jsonrpc-api.md#getslot)
- [getSlotLeader](jsonrpc-api.md#getslotleader)
- [getSlotLeaders](jsonrpc-api.md#getslotleaders)
- [getStakeActivation](jsonrpc-api.md#getstakeactivation)
- [getSupply](jsonrpc-api.md#getsupply)
- [getTokenAccountBalance](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [getTokenLargestAccounts](jsonrpc-api.md#gettokenlargestaccounts)
- [getTokenSupply](jsonrpc-api.md#gettokensupply)
- [getTransaction](jsonrpc-api.md#gettransaction)
- [getTransactionCount](jsonrpc-api.md#gettransactioncount)
- [getVersion](jsonrpc-api.md#getversion)
- [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
- [minimumLedgerSlot](jsonrpc-api.md#minimumledgerslot)
- [requestAirdrop](jsonrpc-api.md#requestairdrop)
- [トランザクションを送信](jsonrpc-api.md#sendtransaction)
- [simulateTransaction](jsonrpc-api.md#simulatetransaction)
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

### Deprecated Methods

- [getConfirmedBlock](jsonrpc-api.md#getconfirmedblock)
- [getConfirmedBlocks](jsonrpc-api.md#getconfirmedblocks)
- [getConfirmedBlocksWithLimit](jsonrpc-api.md#getconfirmedblockswithlimit)
- [getConfirmedSignaturesForAddress2](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [getConfirmedTransaction](jsonrpc-api.md#getconfirmedtransaction)

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

- `"finalized"` - the node will query the most recent block confirmed by supermajority of the cluster as having reached maximum lockout, meaning the cluster has recognized this block as finalized
- `"confirmed"` - the node will query the most recent block that has been voted on by supermajority of the cluster.
  - ゴシップやリプレイによる投票を取り入れています。
  - あるブロックの子孫の票はカウントせず、そのブロックへの直接票のみをカウントします。
  - この確認レベルは、リリース 1.3 以降の"optimistic confirmation"を保証するものでもあります。
- `"processed"` - the node will query its most recent block. なお、ブロックが完全でない場合もあります。

For processing many dependent transactions in series, it's recommended to use `"confirmed"` commitment, which balances speed with rollback safety. For total safety, it's recommended to use`"finalized"` commitment.

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
        "commitment": "finalized"
      }
    ]
  }
'
```

#### デフォルト:

If commitment configuration is not provided, the node will default to `"finalized"` commitment

Bank 状態をクエリするメソッドだけがコミットメントパラメータを受け入れます。 これらは以下の API リファレンスに示されています。

#### RpcResponse 構造

コミットメントパラメータを取る多くのメソッドは、2 つの部分から構成された RpcResponse JSON オブジェクトを返します。

- `context` : 操作が評価された `スロット` フィールドを含む RpcResponseContext JSON 構造。
- `value` : 操作自体が返す値。

## ヘルスチェック

JSON RPC API ではありませんが。 RPC の HTTP エンドポイントの `GET/health` は、ロードバランサや他のネットワークインフラストラクチャで使用するためのヘルスチェックメカニズムを提供します。 This request will always return a HTTP 200 OK response with a body of "ok", "behind" or "unknown" based on the following conditions:

1. If one or more `--trusted-validator` arguments are provided to `solana-validator`, "ok" is returned when the node has within `HEALTH_CHECK_SLOT_DISTANCE` slots of the highest trusted validator, otherwise "behind". "unknown" is returned when no slot information from trusted validators is not yet available.
2. 信頼できるバリデータが提供されていない場合、"ok" は常に返されます。

## JSON RPC API リファレンス

### getAccountInfo

提供されたパブキーのアカウントに関連するすべての情報を返します。

#### パラメータ:

- `<string>`-base-58 エンコード文字列として、クエリするトークンアカウントのパブキー
- `<object>` - (任意) 次のオプションの 項目を含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "base58" は 129 バイト未満のアカウントデータに制限されています。 "base64" は base64 エンコードされたデータを任意のサイズの Account データに対して返します。 "base64+zstd" は、 [Zstandard](https://facebook.github.io/zstd/) を使用してアカウントデータを圧縮し、結果を base64 エンコードします。 "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されていてパーサが見つからない場合、フィールドは "base64" エンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。
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

### getBlock

台帳内の確認済みブロックに関する身元と取引情報を返します。

#### パラメータ:

- `<u64>` - スロット, u64 integer
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". パラメータが指定されていない場合、デフォルトのエンコーディングは "json" です。 "jsonParsed" エンコーディングは、プログラム固有の命令パーサを使用して `transaction.message.instructions` リストに、より人間が読みやすく明示的なデータを返します。 If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### 結果:

結果フィールドは、次のフィールドを持つオブジェクトになります。

- `<null>` - 指定されたブロックが確認されていない場合
- `<object>` - ブロックが確認された場合、次のフィールドを持つオブジェクト:
  - `blockhash: <string>` - base-58 エンコードされた文字列として
  - `previousBlockhash: <string>` - base-58 エンコードされた文字列として、このブロックの親のブロックハッシュ; 元帳のクリーンアップのために親ブロックが使用できない場合、このフィールドは「1111111111111111111111111111111111」を返します。
  - `parentSlot: <u64>` - このブロックの親のスロットインデックス
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
    - `<object>` -トランザクション・ステータス・メタデータ・オブジェクトで、`Null`または
      - `err: <object | null>` -トランザクションが失敗した場合はエラー、トランザクションが成功した場合は NULL です。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - u64 整数としてこのトランザクションに課金された手数料
      - `preBalances: <array>` - トランザクションが処理される前の u64 アカウント残高の配列
      - `postBalances: <array>` - トランザクションが処理された後の u64 アカウント残高の配列
      - `innerInstructions: <array|undefined>` -[内部命令のリスト](#inner-instructions-structure)、またはこのトランザクションで内部命令の記録がまだ有効になっていない場合は省略されます。
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `logMessages: <array>` -ログメッセージの文字列の配列、またはこのトランザクションでログメッセージの記録がまだ有効でない場合は省略されます。
      - DEPRECATED:` status:<object>` - トランザクションステータス
        - `"Ok": <null>` - トランザクションが成功しました
        - `"Err": <ERR>` - TransactionError でトランザクションが失敗しました
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - `pubkey: <string>` - 報酬を受け取ったアカウントの公開鍵（Base-58 でエンコードされた文字列）。
    - `lamports: <i64>`- アカウントにクレジットまたはデビットされた i64 のランポート数。
    - `postBalance: <u64>` -報酬適用後のランポートのアカウント残高
    - `rewardType: <string|undefined>` -報酬の種類 "fee"、"rent"、"voting"、"staking"
  - `blockTime: <i64 | null>` - 推定生産時間 Unix タイムスタンプ(Unix エポックからの秒数) 利用できない場合は null

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
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
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, "base64"]}
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
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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
  - `data: <string>` - base-58 文字列でエンコードされたプログラム入力データ。

#### Token Balances Structure

The JSON structure of token balances is defined as a list of objects in the following structure:

- `accountIndex: <number>` - Index of the account in which the token balance is provided for.
- `mint: <string>` - Pubkey of the token's mint.
- `uiTokenAmount: <object>` -
  - `amount: <string>` - Raw amount of tokens as a string, ignoring decimals.
  - `decimals: <number>` - Number of decimals configured for token's mint.
  - `uiAmount: <number | null>` - Token amount as a float, accounting for decimals. **DEPRECATED**
  - `uiAmountString: <string>` - Token amount as a string, accounting for decimals.

### getBlockProduction

Returns recent block production information from the current or previous epoch.

#### パラメータ:

- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `range: <object>` - Slot range to return block production for. パラメータが指定されていない場合、デフォルトは現在のエポックです。
    - `firstSlot: <u64>` - first slot to return block production information for (inclusive)
    - (optional) `lastSlot: <u64>` - last slot to return block production information for (inclusive). If parameter not provided, defaults to the highest slot
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### 結果:

結果は `値` 以下に等しい RpcResponse JSON オブジェクトになります:

- `<object>`
  - `byIdentity: <object>` - a dictionary of validator identities, as base-58 encoded strings. Value is a two element array containing the number of leader slots and the number of blocks produced.
  - `range: <object>` - Block production slot range
    - `firstSlot: <u64>` - first slot of the block production information (inclusive)
    - `lastSlot: <u64>` - last slot of block production information (inclusive)

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockProduction"}
'
```

結果:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 9887
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [9888, 9886]
      },
      "range": {
        "firstSlot": 0,
        "lastSlot": 9887
      }
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
    "method": "getBlockProduction",
    "params": [
      {
        "identity": "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr",
        "range": {
          "firstSlot": 40,
          "lastSlot": 50
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
      "slot": 10102
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [11, 11]
      },
      "range": {
        "firstSlot": 50,
        "lastSlot": 40
      }
    }
  },
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

### getBlocks

2 つのスロット間の確認済みブロックのリストを返します

#### パラメータ:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - (任意) end_slot, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### 結果:

結果フィールドは、`start_slot`から`end_slot`（指定されている場合）までの間に確認されたブロック、または最新の確認されたブロックを含む、u64 整数の配列となります。 許容最大範囲は 500,000 スロットです。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocks","params":[5, 10]}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": [5, 7, 8, 9, 10], "id": 1 }
```

### getBlocksWithLimit

指定されたスロットから始まる確認されたブロックのリストを返します

#### パラメータ:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### 結果:

Result フィールドは、確認されたブロックを `start_slot` から `limit` ブロックまでリストする u64 整数の配列になります。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocksWithLimit","params":[5, 3]}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getBlockTime

Returns the estimated production time of a block.

各バリデータは、特定のブロックの投票にタイムスタンプを追加することで、UTC 時間を台帳に報告します。 要求されたブロックタイムは、台帳に記録された一連の最近のブロックの中で、投票タイムスタンプのステーキング荷重平均から計算されます。

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
- `lastValidSlot: <u64>` - DEPRECATED - this value is inaccurate and should not be relied upon

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
- `テーパー: <f64>`, インフレ率が引き下げられる年. Rate reduction is derived using the target slot time in genesis config
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

### getInflationReward

Returns the inflation reward for a list of addresses for an epoch

#### パラメータ:

- `<array>` - An array of addresses to query, as base-58 encoded strings

* `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `epoch: <u64>` - An epoch for which the reward occurs. If omitted, the previous epoch will be used

#### 結果

The result field will be a JSON array with the following fields:

- `epoch: <u64>`, epoch for which reward occured
- `effectiveSlot: <u64>`, the slot in which the rewards are effective
- `amount: <u64>`, reward amount in lamports
- `postBalance: <u64>`, post balance of the account in lamports

#### 例

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getInflationReward",
    "params": [
       ["6dmNQ5jwLeLk5REvio1JcMshcbvkYMwy26sJ8pbkvStu", "BGsqMegLpV6n6Ve146sSX2dTjUMj3M92HnU8BbNRMhF2"], 2
    ]
  }
'
```

応答:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "amount": 2500,
      "effectiveSlot": 224,
      "epoch": 2,
      "postBalance": 499999442500
    },
    null
  ],
  "id": 1
}
```

### getLargestAccounts

Returns the 20 largest accounts, by lamport balance (results may be cached up to two hours)

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
- `<object>` - (任意) 次のフィールドを含む設定オブジェクト:
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### 結果:

- `<null>` - 要求されたエポックが見つからない場合
- `<object>` - otherwise, the result field will be a dictionary of validator identities, as base-58 encoded strings, and their corresponding leader slot indices as values (indices are relative to the first slot in the requested epoch)

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

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getLeaderSchedule",
    "params": [
      null,
      {
        "identity": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
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

### getMaxRetransmitSlot

Get the max slot seen from retransmit stage.

#### 結果:

- `<u64>` - スロット

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxRetransmitSlot"}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getMaxShredInsertSlot

Get the max slot seen from after shred insert.

#### 結果:

- `<u64>` - スロット

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxShredInsertSlot"}
'
```

結果:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
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
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "base58" is limited to Account data of less than 129 bytes. "base64" は base64 エンコードされたデータを任意のサイズの Account データに対して返します。 "base64+zstd" は、 [Zstandard](https://facebook.github.io/zstd/) を使用してアカウントデータを圧縮し、結果を base64 エンコードします。 "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されていてパーサが見つからない場合、フィールドは "base64" エンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。
  - (オプション) `dataSlice: <object>` - 指定された `オフセット: <usize>` と `長さ: <usize>` フィールド; "base58", "base64" または "base64+zstd" エンコーディングでのみ使用できます。

#### 結果:

結果は `値` 以下に等しい RpcResponse JSON オブジェクトになります:

以下の配列:

- `<null>` - その Pubkey のアカウントが存在しない場合
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
  - `encoding: <string>` - "base58" (_slow_), "base64", "base64+zstd", "jsonParsed". "base58" is limited to Account data of less than 129 bytes. "base64" は base64 エンコードされたデータを任意のサイズの Account データに対して返します。 "base64+zstd" は、 [Zstandard](https://facebook.github.io/zstd/) を使用してアカウントデータを圧縮し、結果を base64 エンコードします。 "jsonParsed"エンコーディングは、プログラム固有の状態パーサを使用して、より人が読める明示的なアカウント状態データを返す試みです。 "jsonParsed" が要求されていてパーサが見つからない場合、フィールドは "base64" エンコーディングに戻ります。 `データ` フィールドが `<string>` のときに検出可能です。
  - (オプション) `dataSlice: <object>` - 指定された `オフセット: <usize>` と `長さ: <usize>` フィールド; "base58", "base64" または "base64+zstd" エンコーディングでのみ使用できます。
  - (任意) `filters: <array>` - さまざまな [フィルター オブジェクト](jsonrpc-api.md#filters)を使用したフィルター結果。 アカウントは、結果に含めるすべてのフィルタ条件を満たしている必要があります

##### フィルター:

- `memcmp: <object>` - 特定のオフセット時に提供される一連のバイトとプログラムアカウントデータを比較する。 フィールド:

  - `offset: <usize>` - 比較を開始するためのプログラムアカウントデータのオフセット
  - `bytes: <string>` - data to match, as base-58 encoded string and limited to less than 129 bytes

- `dataSize: <u64>` - プログラムアカウントのデータ長と提供されるデータサイズを比較します。

#### 結果:

Result フィールドは JSON オブジェクトの配列になり、以下のようになります。

- `pubkey: <string>` - Base-58 でエンコードされた文字列としてのアカウントパブキー
- `account: <object>` - 次のサブフィールドを持つ JSON オブジェクト:
  - `lamports: <u64>`, このアカウントに割り当てられたラムポートの数, u64 として
  - `owner:<string>`このアカウントが割り当てられているプログラムの Base-58 でエンコードされたパブキー。 `data: <[string,encoding]|object>`, アカウントに関連するデータで、エンコードされたバイナリデータまたはエンコードパラメータ依存の JSON 形式 `{<program>: <state>}`。
  - `executable: <bool>`, アカウントにプログラム \(および厳密には読み取り専用\) が含まれているかを示す boolean
  - `rentEpoch: <u64>`, u64 として、このアカウントが賃貸借義務を負うエポック

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

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

文字列ブロックハッシュと"FeeCalculator JSON"オブジェクトで構成される"JSON オブジェクト"を含む RpcResponse

- `RpcResponse<object>` - JSON オブジェクトを含む `値` フィールドを持つ RpcResponse JSON オブジェクト
- `blockhash: <string>` - base-58 エンコードされた文字列としてのハッシュ
- `feeCalculator: <object>` - FeeCalculator オブジェクト、このブロックハッシュの手数料スケジュール。

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
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

### getSignaturesForAddress

提供された署名または直近に確認されたブロックから、アドレスを逆方向に含むトランザクションの確認済みの署名を返します。

#### パラメータ:

- `<string>` - base-58 エンコードされた文字列としてのアカウント アドレス
- `<object>` - (任意) 次のフィールドを含む設定オブジェクト。
  - `limit: <number>` - (任意) 返す最大トランザクション署名数(1 から 1,000、デフォルト：1,000)
  - `before: <string>` - (任意) このトランザクション署名から逆方向の検索を開始します。 指定されていない場合は、最も高い確認されたブロックの上から検索が開始されます。
  - `until: <string>` - (任意) このトランザクション署名に達するまで検索します。
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### 結果:

Result フィールドは、トランザクション署名情報の配列を最新から最古のトランザクションに注文します。

- `<object>`
  - `signature: <string>` - base-58 エンコードされた文字列としてのトランザクション署名
  - `slot: <u64>` - トランザクションを含むブロックを含むスロット
  - `err: <object | null>` -トランザクションが失敗した場合はエラー、トランザクションが成功した場合は NULL です。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - トランザクションに関連付けられたメモ メモがない場合は null。
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### 例:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
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
      "slot": 114,
      "blockTime": null
    }
  ],
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
  - `err: <object | null>` -トランザクションが失敗した場合はエラー、トランザクションが成功した場合は NULL です。 [TransactionError definition](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus: <string | null>` - トランザクションのクラスタ確認ステータス; `processed`, `confirmed`, `finalized`. 楽観的な確認については、 [コミット](jsonrpc-api.md#configuring-state-commitment) を参照してください。
  - DEPRECATED:` status:<object>` - トランザクションステータス
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

結果：

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

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

結果:

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

### getSlotLeaders

Returns the slot leaders for a given slot range

#### パラメータ:

- `<u64>` - Start slot, as u64 integer
- `<u64>` - Limit, as u64 integer

#### 結果:

- `<array<string>>` - Node identity public keys as base-58 encoded strings

#### 例:

If the current slot is #99, query the next 10 leaders with the following request:

リクエスト:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeaders", "params":[100, 10]}
'
```

結果:

The first leader returned is the leader for slot #100:

```json
{
  "jsonrpc": "2.0",
  "result": [
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP"
  ],
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

結果:

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

結果:

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

SPL トークンアカウントのトークン残高を返します。

#### パラメータ:

- `<string>` - Pubkey of Token account to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `amount: <string>` - the raw balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place
- `uiAmount: <number | null>` - the balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the balance as a string, using mint-prescribed decimals

#### 例:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStEuGbc3sM84cKRL6yYaaSstyLSU4ve5oovLS7"]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "amount": "9864",
      "decimals": 2,
      "uiAmount": 98.64,
      "uiAmountString": "98.64"
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

Returns all SPL Token accounts by approved Delegate.

#### パラメータ:

- `<string>` - Pubkey of account delegate to query, as base-58 encoded string
- `<object>` - Either:
  - `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  - `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### 結果:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

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
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
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

Returns all SPL Token accounts by token owner.

#### パラメータ:

- `<string>` - Pubkey of account owner to query, as base-58 encoded string
- `<object>` - Either:
  - `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  - `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### 結果:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

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

Result:

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
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
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

Returns the 20 largest accounts of a particular SPL Token type.

#### パラメータ:

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects containing:

- `address: <string>` - the address of the token account
- `amount: <string>` - the raw token account balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place
- `uiAmount: <number | null>` - the token account balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the token account balance as a string, using mint-prescribed decimals

#### 例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Result:

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
        "uiAmount": 7.71,
        "uiAmountString": "7.71"
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29,
        "uiAmountString": "2.29"
      }
    ]
  },
  "id": 1
}
```

### getTokenSupply

Returns the total supply of an SPL Token type.

#### パラメータ:

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 結果:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `amount: <string>` - the raw total token supply without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place
- `uiAmount: <number | null>` - the total token supply, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the total token supply as a string, using mint-prescribed decimals

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
      "amount": "100000",
      "decimals": 2,
      "uiAmount": 1000,
      "uiAmountString": "1000"
    }
  },
  "id": 1
}
```

### getTransaction

Returns transaction details for a confirmed transaction

#### パラメータ:

- `<string>` - transaction signature as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### 結果:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null if not available
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### 例:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
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
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
  "blockTime": null,
  "id": 1
}
```

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
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
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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

### getTransactionCount

Returns the current Transaction count from the ledger

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<u64>` - count

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

Result:

```json
{ "jsonrpc": "2.0", "result": 268, "id": 1 }
```

### getVersion

Returns the current solana versions running on the node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `solana-core`, software version of solana-core
- `feature-set`, unique identifier of the current software's feature set

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": { "solana-core": "1.7.0" }, "id": 1 }
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

- `<object>` - (optional) Configuration object containing the following field:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `votePubkey: <string>` - Only return results for this validator vote address (base-58 encoded)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

- `votePubkey: <string>` - Vote account address, as base-58 encoded string
- `nodePubkey: <string>` - Validator identity, as base-58 encoded string
- `activatedStake: <u64>` - the stake, in lamports, delegated to this vote account and active in this epoch
- `epochVoteAccount: <bool>` - bool, whether the vote account is staked for this epoch
- `commission: <number>`, percentage (0-100) of rewards payout owed to the vote account
- `lastVote: <u64>` - Most recent slot voted on by this vote account
- `epochCredits: <array>` - History of how many credits earned by the end of each epoch, as an array of arrays containing: `[epoch, credits, previousCredits]`

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

結果：

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

#### Example: Restrict results to a single validator vote account

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getVoteAccounts",
    "params": [
      {
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
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
    "delinquent": []
  },
  "id": 1
}
```

### minimumLedgerSlot

Returns the lowest slot that the node has information about in its ledger. This value may increase over time if the node is configured to purge older ledger data

#### Parameters:

該当なし

#### Results:

- `u64` - Minimum ledger slot

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

結果：

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### requestAirdrop

Requests an airdrop of lamports to a Pubkey

#### Parameters:

- `<string>` - Pubkey of account to receive lamports, as base-58 encoded string
- `<integer>` - lamports, as a u64
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (used for retrieving blockhash and verifying airdrop success)

#### Results:

- `<string>` - Transaction Signature of airdrop, as base-58 encoded string

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 1000000000]}
'

```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
  "id": 1
}
```

### sendTransaction

Submits a signed transaction to the cluster for processing.

This method does not alter the transaction in any way; it relays the transaction created by clients to the node as-is.

If the node's rpc service receives the transaction, this method immediately succeeds, without waiting for any confirmations. A successful response from this method does not guarantee the transaction is processed or confirmed by the cluster.

While the rpc service will reasonably retry to submit it, the transaction could be rejected if transaction's `recent_blockhash` expires before it lands.

Use [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) to ensure a transaction is processed and confirmed.

Before submitting, the following preflight checks are performed:

1. トランザクション署名が確認されました。
2. このトランザクションは、"preflight"コミットメントによって指定された銀行スロットに対してシミュレートされます。 失敗した場合はエラーが返されます。 必要に応じてプリフライトチェックは無効になっている可能性があります。 混乱する振る舞いを避けるために、同じコミットメントと事前コミットメントを指定することをお勧めします。

The returned signature is the first signature in the transaction, which is used to identify the transaction ([transaction id](../../terminology.md#transanction-id)). This identifier can be easily extracted from the transaction data before submission.

#### Parameters:

- `<string>` - fully-signed Transaction, as encoded string
- `<object>` - (optional) Configuration object containing the following field:
  - `skipPreflight: <bool>` - if true, skip the preflight transaction checks (default: false)
  - `preflightCommitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to use for preflight (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

- `<string>` - First Transaction Signature embedded in the transaction, as base-58 encoded string ([transaction id](../../terminology.md#transanction-id))

#### Example:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb",
  "id": 1
}
```

### simulateTransaction

Simulate sending a transaction

#### Parameters:

- `<string>` - Transaction, as an encoded string. The transaction must have a valid blockhash, but is not required to be signed.
- `<object>` - (optional) Configuration object containing the following field:
  - `sigVerify: <bool>` - if true the transaction signatures will be verified (default: false)
  - `commitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to simulate the transaction at (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

An RpcResponse containing a TransactionStatus object The result will be an RpcResponse JSON object with `value` set to a JSON object with the following fields:

- `err: <object | string | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - Array of log messages the transaction instructions output during execution, null if simulation failed before the transaction was able to execute (for example due to an invalid blockhash or signature verification failure)

#### Example:

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

Result:

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

## Subscription Websocket

After connecting to the RPC PubSub websocket at `ws://<ADDRESS>/`:

- Submit subscription requests to the websocket using the methods below
- Multiple subscriptions may be active at once
- Many subscriptions take the optional [`commitment` parameter](jsonrpc-api.md#configuring-state-commitment), defining how finalized a change should be to trigger a notification. For subscriptions, if commitment is unspecified, the default value is `"finalized"`.

### accountSubscribe

Subscribe to an account to receive notifications when the lamports or data for a given account public key changes

#### Parameters:

- `<string>` - account Pubkey, as base-58 encoded string
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to binary encoding, detectable when the `data` field is type `<string>`.

#### Results:

- `<number>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 23784, "id": 1 }
```

#### Notification Format:

Base58 encoding:

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

Parsed-JSON encoding:

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

### accountUnsubscribe

Unsubscribe from account change notifications

#### Parameters:

- `<number>` - id of account Subscription to cancel

#### Results:

- `<bool>` - 登録解除成功メッセージ

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "accountUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### logsSubscribe

Subscribe to transaction logging

#### Parameters:

- `filter: <string>|<object>` - filter criteria for the logs to receive results by account type; currently supported:
  - "all" - subscribe to all transactions except for simple vote transactions
  - "allWithVotes" - subscribe to all transactions including simple vote transactions
  - `{ "mentions": [ <string> ] }` - subscribe to all transactions that mention the provided Pubkey (as base-58 encoded string)
- `<object>` - (任意) 次のオプションフィールドを含む設定オブジェクト。
  - (オプション) [コミットメント](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<integer>` - サブスクリプション ID \(購読解除に必要)

#### Example:

Request:

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
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

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

Unsubscribe from transaction logging

#### Parameters:

- `<integer>` - id of subscription to cancel

#### Results:

- `<bool>` - 登録解除成功メッセージ

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "logsUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### programSubscribe

Subscribe to a program to receive notifications when the lamports or data for a given account owned by the program changes

#### Parameters:

- `<string>` - program_id Pubkey, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to base64 encoding, detectable when the `data` field is type `<string>`.
  - (optional) `filters: <array>` - filter results using various [filter objects](jsonrpc-api.md#filters); account must meet all filter criteria to be included in results

#### Results:

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

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

Parsed-JSON encoding:

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

Unsubscribe from program-owned account change notifications

#### Parameters:

- `<integer>` - id of account Subscription to cancel

#### Results:

- `<bool>` - 登録解除成功メッセージ

#### Example:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programUnsubscribe", "params": [0] }
```

結果:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### signatureSubscribe

Subscribe to a transaction signature to receive notification when the transaction is confirmed On `signatureNotification`, the subscription is automatically cancelled

#### Parameters:

- `<string>` - Transaction Signature, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

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
      "commitment": "finalized"
    }
  ]
}
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

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

Unsubscribe from signature confirmation notification

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "signatureUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### slotSubscribe

Subscribe to receive notification anytime a slot is processed by the validator

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

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

Unsubscribe from slot notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotUnsubscribe", "params": [0] }
```

結果:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### rootSubscribe

Subscribe to receive notification anytime a new root is set by the validator.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

リクエスト:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest root slot number.

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

### rootUnsubscribe

Unsubscribe from root notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### voteSubscribe - Unstable, disabled by default

**This subscription is unstable and only available if the validator was started with the `--rpc-pubsub-enable-vote-subscription` flag. The format of this subscription may change in the future**

Subscribe to receive notification anytime a new vote is observed in gossip. These votes are pre-consensus therefore there is no guarantee these votes will enter the ledger.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest vote, containing its hash, a list of voted slots, and an optional timestamp.

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

### voteUnsubscribe

Unsubscribe from vote notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteUnsubscribe", "params": [0] }
```

Response:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

## JSON RPC API Deprecated Methods

### getConfirmedBlock

**DEPRECATED: Please use [getBlock](jsonrpc-api.md#getblock) instead** This method is expected to be removed in solana-core v1.8

Returns identity and transaction information about a confirmed block in the ledger

#### Parameters:

- `<u64>` - slot, as u64 integer
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an object with the following fields:

- `<null>` - if specified block is not confirmed
- `<object>` - if block is confirmed, an object with the following fields:
  - `blockhash: <string>` - the blockhash of this block, as base-58 encoded string
  - `previousBlockhash: <string>` - the blockhash of this block's parent, as base-58 encoded string; if the parent block is not available due to ledger cleanup, this field will return "11111111111111111111111111111111"
  - `parentSlot: <u64>` - the slot index of this block's parent
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
    - `meta: <object>` - transaction status metadata object, containing `null` or:
      - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - fee this transaction was charged, as u64 integer
      - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
      - `postBalances: <array>` - array of u64 account balances after the transaction was processed
      - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
      - DEPRECATED: `status: <object>` - Transaction status
        - `"Ok": <null>` - Transaction was successful
        - `"Err": <ERR>` - Transaction failed with TransactionError
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - `pubkey: <string>` - The public key, as base-58 encoded string, of the account that received the reward
    - `lamports: <i64>`- number of reward lamports credited or debited by the account, as a i64
    - `postBalance: <u64>` - account balance in lamports after the reward was applied
    - `rewardType: <string|undefined>` - type of reward: "fee", "rent", "voting", "staking"
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch). null if not available

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

Result:

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
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

For more details on returned data: [Transaction Structure](jsonrpc-api.md#transactionstructure) [Inner Instructions Structure](jsonrpc-api.md#innerinstructionsstructure) [Token Balances Structure](jsonrpc-api.md#tokenbalancesstructure)

### getConfirmedBlocks

**DEPRECATED: Please use [getBlocks](jsonrpc-api.md#getblocks) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks between two slots

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - (optional) end_slot, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks between `start_slot` and either `end_slot`, if provided, or latest confirmed block, inclusive. Max range allowed is 500,000 slots.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### getConfirmedBlocksWithLimit

**DEPRECATED: Please use [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks starting at the given slot

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks starting at `start_slot` for up to `limit` blocks, inclusive.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getConfirmedSignaturesForAddress2

**DEPRECATED: Please use [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress) instead** This method is expected to be removed in solana-core v1.8

Returns confirmed signatures for transactions involving an address backwards in time from the provided signature or most recent confirmed block

#### Parameters:

- `<string>` - account address as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following fields:
  - `limit: <number>` - (optional) maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
  - `before: <string>` - (optional) start searching backwards from this transaction signature. If not provided the search starts from the top of the highest max confirmed block.
  - `until: <string>` - (optional) search until this transaction signature, if found before limit reached.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of transaction signature information, ordered from newest to oldest transaction:

- `<object>`
  - `signature: <string>` - transaction signature as base-58 encoded string
  - `slot: <u64>` - The slot that contains the block with the transaction
  - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - Memo associated with the transaction, null if no memo is present
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114,
      "blockTime": null
    }
  ],
  "id": 1
}
```

### getConfirmedTransaction

**DEPRECATED: Please use [getTransaction](jsonrpc-api.md#gettransaction) instead** This method is expected to be removed in solana-core v1.8

Returns transaction details for a confirmed transaction

#### Parameters:

- `<string>` - transaction signature as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null if not available
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
  "blockTime": null,
  "id": 1
}
```

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
