---
title: JSON RPC API
---

Solana节点使用[JSON-RPC 2.0](https://www.jsonrpc.org/specification)规范接受HTTP请求。

要与JavaScript应用程序中的Solana节点进行交互，请使用[solana-web3.js](https://github.com/solana-labs/solana-web3.js)库，该库为RPC方法提供了方便的接口。

## RPC HTTP 端点

**Default port:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## RPC PubSub WebSocket 端点

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## 方法

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
- [getFeeRateGovernor](jsonrpc-api.md#getfeerategovernor)
- [getFees](jsonrpc-api.md#getfees)
- [getFirstAvailableBlock](jsonrpc-api.md#getfirstavailableblock)
- [getGenesisHash](jsonrpc-api.md#getgenesishash)
- [getHealth](jsonrpc-api.md#gethealth)
- [getIdentity](jsonrpc-api.md#getidentity)
- [getInflationGovernor](jsonrpc-api.md#getinflationgovernor)
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
- [sendTransaction](jsonrpc-api.md#sendtransaction)
- [simulateTransaction](jsonrpc-api.md#simulatetransaction)
- [setLogFilter](jsonrpc-api.md#setlogfilter)
- [validatorExit](jsonrpc-api.md#validatorexit)
- [Subscription](jsonrpc-api.md#subscription-websocket)
  - [accountSubscribe](jsonrpc-api.md#accountsubscribe)
  - [accountUnsubscribe](jsonrpc-api.md#accountunsubscribe)
  - [logsSubscribe](jsonrpc-api.md#logssubscribe)
  - [logsUnsubscribe](jsonrpc-api.md#logsunsubscribe)
  - [programSubscribe](jsonrpc-api.md#programsubscribe)
  - [programUnsubscribe](jsonrpc-api.md#programunsubscribe)
  - [signatureSubscribe](jsonrpc-api.md#signaturesubscribe)
  - [signatureUnsubscribe](jsonrpc-api.md#signatureunsubscribe)
  - [slotSubscribe](jsonrpc-api.md#slotsubscribe)
  - [slotUnsubscribe](jsonrpc-api.md#slotunsubscribe)

## 不稳定的方法

不稳定的方法可能会在补丁程序发行版中发生重大更改，并且可能永久不受支持。

- [getTokenAccountBalance](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [getTokenLargestAccounts](jsonrpc-api.md#gettokenlargestaccounts)
- [getTokenSupply](jsonrpc-api.md#gettokensupply)

## 请求格式

要发出 JSON-RPC 请求，请发送带有`Content-Type:
application/json`的 HTTP POST 请求。 JSON请求数据应包含4个字段：

- `jsonrpc: <string>`，设置为 `"2.0"`
- `id： <number>`，一个独特的客户端生成的识别整数
- `method: <string>`，一个包含要调用方法的字符串
- `params: <array>`，一个 JSON 数组的有序参数值

使用curl的示例：

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

响应输出将是一个 JSON 对象，具有以下字段：

- `jsonrpc: <string>`, 匹配请求规范
- `id: <number>`, 匹配请求标识符
- `result: <array|number|object|string>`, 请求的数据或成功确认

通过发送JSON-RPC请求对象数组作为单个POST的数据，可以批量发送请求。

## 定义

- 哈希（Hash）：一个数据块的SHA-256哈希。
- 公钥（Pubkey）：Ed25519密钥对的公钥。
- 交易（Transaction）：由客户密钥对签名以授权这些操作的Solana指令列表。
- 签名（Signature）：交易的有效载荷数据的Ed25519签名，包括指令。 它可以用来识别交易。

## 配置状态承诺

对于飞行前检查和交易处理，Solana节点根据客户端设置的承诺要求选择要查询的银行状态。 该承诺描述了该时间点块的最终确定方式。  查询账本状态时，建议使用较低级别的承诺来报告进度，而使用较高级别以确保不会回滚该状态。

客户可以按照承诺的降序排列(从最高确定到最低确定)：

- `"max"` - 节点将查询由集群的绝大多数确认为已达到最大锁定的最新块，这意味着集群已将该块识别为已完成
- `"root"` - 节点将查询该节点上已达到最大锁定的最新块，这意味着该节点已将该块识别为已完成
- `"singleGossip"` - 节点将查询由集群的多数投票的最新区块。
  - 它融合了八卦和重播的选票。
  - 它不计算该区块后代的票数，而仅对该区块的直接票数进行计数。
  - 此确认级别还支持1.3版及更高版本中的“乐观确认”保证。
- `"recent"` - 节点将查询其最近的块。  注意，该区块可能不完整。

为了连续处理许多相关的事务，建议使用`"singleGossip"`承诺，以平衡速度和回滚安全性。 为了安全起见，建议使用`"max"`承诺。

#### 示例：

承诺参数应作为 `params` 数组中的最后一个元素包含：

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

#### 默认情况：

如果未提供承诺配置，则该节点将默认为`max`承诺

只有查询库状态的方法才接受承诺参数。 它们在下面的API参考中指出。

#### RpcResponse结构

许多采用承诺参数的方法会返回RpcResponse JSON对象，该对象由两部分组成：

- `context` : RpcResponseContext JSON结构，包括一个`slot`字段，在该字段上评估操作。
- `value` ：操作本身返回的值。

## 健康检查

尽管不是JSON RPC API，但RPC HTTP端点上的`GET / health`提供了一种健康检查机制，供负载平衡器或其他网络基础结构使用。 根据以下条件，此请求将始终返回带有 "ok" 或 "behind" 正文的 HTTP 200 OK 响应：

1. 如果向`solana-validator`提供了一个或多个`--trusted-validator`参数，则当节点位于最高可信验证器的`HEALTH_CHECK_SLOT_DISTANCE`插槽内时，返回 "ok"，否则返回 "behind"。
2. 如果未提供受信任的验证器，则始终返回 "ok"。

## JSON RPC API 引用

### getAccountInfo

返回与提供的Pubkey帐户关联的所有信息

#### 参数：

- `<string>` - 要查询的帐户的公钥，以base-58编码的字符串
- `<object>` -(可选)包含以下可选字段的配置对象：
  - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)
  - `encoding：<string>` - 帐户数据的编码，可以是"base58"(*很慢*)，"base64"，"base64+zstd" 或 "jsonParsed"。 "base58" 仅限于少于128个字节的帐户数据。 "base64" 将为任何大小的Account数据返回base64编码的数据。 "base64+zstd" 使用 [Zstandard](https://facebook.github.io/zstd/) 压缩帐户数据，并对结果进行base64编码。 "jsonParsed" 编码尝试使用特定于程序的状态解析器来返回更多的人类可读和显式的帐户状态数据。 如果请求了 "jsonParsed"，但找不到解析器，则该字段回退为"base64"编码，当`data`字段为`<string>`类型时可以检测到。
  - (可选) `dataSlice：<object>` - 使用提供的`offset：<usize>` 和`length：<usize>`字段限制返回的帐户数据；仅适用于 "base58"，"base64" 或 "base64+zstd" 编码。

#### 结果：

结果是一个RpcResponse JSON对象，其`值`等于：

- `<null>` - 如果所请求的帐户不存在
- `<object>` - 否则为JSON对象，其中包含：
  - `lamports: <u64>`，分配给此帐户的Lamport数量，以u64表示
  - `owner: <string>`，此帐户已分配给该程序的base-58编码的Pubkey
  - `data: <[string, encoding]|object>`，与帐户关联的数据，可以是编码的二进制数据，也可以是JSON格式的`{<program>: <state>}`，具体取决于编码参数
  - `executable: <bool>`，布尔值，指示帐户是否包含程序\(并且严格为只读\)
  - `rentEpoch: <u64>`，此帐户下一次将要欠租金的时期，即u64

#### 示例:

请求：
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
响应：
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

#### 示例：
请求：
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
响应：
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

返回提供的 Pubkey 账户余额

#### 参数：

- `<string>` - 要查询的帐户的公钥，以base-58编码的字符串
- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `RpcResponse<u64>` - RpcResponse JSON 对象有 `值` 字段设置为余额

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

结果：
```json
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":0},"id":1}
```

### getBlockCommitment

返回特定区块的承诺

#### 参数：

- `<u64>` - 由插槽指定的区块

#### 结果：

结果字段将是一个 JSON 对象，其中包含：

- `commitment` - 承诺，包括以下任何一项：
  - `<null>` - 未知区块
  - `<array>` - 承诺，在每个深度从 0 到 `MAX_LOCKOUT_HISTORY` + 1 上投票的lamports中的集群质押数量
- `totalStake` - 当前epoch的全部活跃质押（以lamports计算）

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

结果：
```json
{
  "jsonrpc":"2.0",
  "result":{
    "commitment":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,10,32],
    "totalStake": 42
  },
  "id":1
}
```

### getBlockTime

返回已确认块的估计生产时间。

每个验证者通过向特定块的投票间歇性地添加时间戳，定期将其UTC时间报告给账本。 根据记录在账本上的一组最近区块中的Vote时间戳的权益加权平均值计算请求区块的时间。

从快照引导或限制账本大小(通过清除旧插槽) 引导的节点将返回其最低根+ `TIMESTAMP_SLOT_RANGE`以下的块的空时间戳。 对拥有此历史数据感兴趣的用户必须查询根据起源建立的节点，并保留整个账本。

#### 参数：

- `<u64>` - 由插槽指定的区块

#### 结果：

* `<i64>` - 估计生产时间为 Unix 时间戳 (自Unix epoch以来的秒数)
* `<null>` - 此区块不可用的时间戳

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockTime","params":[5]}
'
```

结果：
```json
{"jsonrpc":"2.0","result":1574721591,"id":1}
```

### getClusterNodes

返回所有参与集群节点的信息

#### 参数：

无

#### 结果：

结果字段将是一个 JSON 对象的数组，每个子字段如下：

- `pubkey： <string>` - 节点公钥作为基本58编码字符串
- `gossip: <string>` - 节点的 Gossip 网络地址
- `tpu: <string>` - 节点的 TPU 网络地址
- `rpc: <string>|null` - 节点的 JSON RPC 网络地址，或 `null` 如果未启用 JSON RPC 服务
- `version: <string>|null` - 节点的软件版本，或 `null` 如果版本信息不可用

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}
'
```

结果：
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

返回账本中已确认区块的身份和交易信息

#### 参数：

- `<u64>` - 作为u64整数
- `<string>` - 为每个返回的交易编码，或者是"json", "jsonParsed", "base58"(*慢*), "base64"。 如果参数未提供，默认编码为“json”。 "jsonParsed"编码尝试使用针对特定程序的教学解析器返回在 `transaction.message.instruction` 列表中更易读和更明确的数据。 如果“jsonParsed”是请求的，但无法找到解析器，则该指令返回到正则JSON编码(`帐户`, `数据`和 `程序 ID 索引` 字段).

#### 结果：

结果字段将是具有以下字段的对象：

- `<null>` - 如果指定的区块未确认
- `<object>` - 如果区块得到确认，则具有以下字段的对象：
  - `blockhash: <string>` - 该区块的区块哈希作为基准-58 编码字符串
  - `previousBlockhash: <string>` - 该区块的父级区块哈希，以base-58编码的字符串；如果由于账本清理而导致父块不可用，则此字段将返回“11111111111111111111111111111111”
  - `parentSlot: <u64>` - 该区块的父级插槽索引
  - `transactions: <array>` - 包含以下内容的JSON对象数组：
    - `transaction: <object|[string,encoding]>` - [交易](#transaction-structure) 对象，采用JSON格式或已编码的二进制数据，具体取决于编码参数
    - `meta: <object>` - 交易状态元数据对象，包含`null`或：
      - `err: <object | null>` - 如果交易失败，则返回错误；如果交易成功，则返回null。 [TransactionError定义](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - 该交易收取的费用，以u64整数表示
      - `preBalances: <array>` - 处理交易之前的u64帐户余额数组
      - `postBalances: <array>` - 处理交易后的u64帐户余额数组
      - `innerInstructions: <array|undefined>` -[内部指令](#inner-instructions-structure)的列表，如果在此事务处理期间尚未启用内部指令记录，则将其省略
      - `logMessages: <array>` - 字符串日志消息的数组；如果在此事务期间尚未启用日志消息记录，则将其省略
      - DEPRECATED: `status: <object>` - 交易状态
        - `"Ok": <null>` - 交易成功
        - `"Err": <ERR>` - 事务失败，出现TransactionError
  - `rewards: <array>` - 包含以下内容的JSON对象数组：
    - `pubkey: <string>` - 接收奖励的帐户的公钥，以base-58编码的字符串
    - `lamports: <i64>`- 帐户贷记或借记的奖励灯饰的数量，作为i64
    - `postBalance: <u64>` - 应用奖励后以Lamports为单位的帐户余额
    - `rewardType: <string|undefined>` - 奖励类型：“费用”，“租金”，“投票”，“赌注”
  - `blockTime: <i64 | null>` - 估计的生产时间，以Unix时间戳记(自Unix时代以来的秒数)。 如果不可用，则返回null

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}
'
```

结果：
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
          "postBalances": [
            499998932500,
            26858640,
            1,
            1,
            1
          ],
          "preBalances": [
            499998937500,
            26858640,
            1,
            1,
            1
          ],
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
                "accounts": [
                  1,
                  2,
                  3,
                  0
                ],
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

#### 示例:
请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

结果：
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
          "postBalances": [
            499998932500,
            26858640,
            1,
            1,
            1
          ],
          "preBalances": [
            499998937500,
            26858640,
            1,
            1,
            1
          ],
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

#### 交易结构

交易与其他区块链上的交易有很大不同。 请务必阅读[交易解剖](developing/programming-model/transactions.md)，以了解Solana上的交易。

交易的JSON结构定义如下：

- `signatures: <array[string]>` - 应用于交易的以base-58编码的签名的列表。 该列表的长度始终为`message.header.numRequiredSignatures`，并且不为空。 索引`i”`处的签名对应于`message.account_keys`中索引`i`处的公钥。 第一个用作[交易ID](../../terminology.md#transaction-id)。
- `message: <object>` - 定义交易的内容。
  - `accountKeys: <array[string]>` - 交易使用的base-58编码公共密钥列表，包括指令和签名。 第一个`message.header.numRequiredSignatures`公钥必须对交易进行签名。
  - `header: <object>` - 详细说明交易所需的帐户类型和签名。
    - `numRequiredSignatures: <number>` - 使交易有效所需的签名总数。 签名必须与`message.account_keys`的第一个`numRequiredSignatures`匹配。
    - `numReadonlySignedAccounts: <number>` - 签名密钥的最后一个`numReadonlySignedAccounts`是只读帐户。 程序可以处理多个事务，这些事务在单个PoH条目中加载只读帐户，但不允许贷记或借记Lamport或修改帐户数据。 顺序评估针对同一读写帐户的交易。
    - `numReadonlyUnsignedAccounts: <number>` - 未签名密钥的最后一个`numReadonlyUnsignedAccounts`是只读帐户。
  - `recentBlockhash: <string>` - 账本中最近区块的基数为58的编码哈希，用于防止交易重复并延长交易寿命。
  - `instructions: <array[object]>` - 程序指令的列表，如果全部成功，这些指令将依次执行并在一次原子事务中提交。
    - `programIdIndex: <number>` - 在`message.accountKeys`数组中的索引，指示执行该指令的程序帐户。
    - `accounts: <array[number]>` - `message.accountKeys`数组中的有序索引列表，指示要传递给程序的帐号。
    - `data: <string>` - 程序输入的数据以base-58字符串编码。

#### 内部指令结构

Solana运行时记录在事务处理期间调用的跨程序指令，并使这些程序可用，以提高每个事务指令在链上执行的内容的透明度。 调用的指令按原始事务处理指令分组，并按处理顺序列出。

内部指令的JSON结构定义为以下结构中的对象列表：

- `index: number` - 内部指令源自的(一个或多个) 交易指令的索引
- `instructions: <array[object]>` - 内部程序指令的有序列表，在单个事务指令期间被调用。
  - `programIdIndex: <number>` - 在`message.accountKeys`数组中的索引，指示执行该指令的程序帐户。
  - `accounts: <array[number]>` - `message.accountKeys`数组中的有序索引列表，指示要传递给程序的帐号。
  - `data: <string>` - 程序输入的数据以base-58字符串编码。

### getConfirmedBlocks

返回两个插槽之间已确认区块的列表

#### 参数：

- `<u64>` - start_slot，作为u64整数
- `<u64>` - (可选) end_slot，作为u64整数

#### 结果：

结果字段将是一个u64整数数组，其中列出了在`start_slot`和`end_slot`之间（如果提供）或最近确认的块（包括首尾）之间的已确认块。  允许的最大范围是500,000个插槽。


#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

结果：
```json
{"jsonrpc":"2.0","result":[5,6,7,8,9,10],"id":1}
```

### getConfirmedBlocksWithLimit

返回从给定插槽开始的已确认块的列表

#### 参数：

- `<u64>` -start_slot，作为u64整数
- `<u64>` - 限制，如u64整数

#### 结果：

结果字段将是一个u64整数数组，其中列出了已确认的块（从`start_slot`开始），最多到`limit`个块（包括上限）。

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

结果：
```json
{"jsonrpc":"2.0","result":[5,6,7],"id":1}
```

### getConfirmedSignaturesForAddress

**不推荐使用：请改用getConfirmedSignaturesForAddress2**

返回指定槽位范围内涉及地址的事务的所有已确认签名的列表。 允许的最大范围是10,000个插槽

#### 参数：

- `<string>` - 帐户地址为base-58编码的字符串
- `<u64>` - 起始插槽（包含在内）
- `<u64>` -末端插槽（包含在内）

#### 结果：

结果字段将是以下内容的数组：

- `<string>` - 交易签名为以base-58编码的字符串

签名将根据其在其中确认的插槽进行排序，从最低到最高

#### 示例:

请求：
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

结果：
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

从提供的签名或最近确认的块中返回涉及时间在后的地址的交易的确认签名

#### 参数：
* `<string>` - 帐户地址为base-58编码的字符串
* `<object>` - (可选) 包含以下字段的配置对象：
  * `limit: <number>` - (可选) 要返回的最大交易签名 (1到1,000之间，默认值：1,000)。
  * `before: <string>` -(可选) 从此事务签名开始向后搜索。 如果未提供，则从最大已确认最大块的顶部开始搜索。
  * `until: <string>` - (可选) 搜索直到此交易签名，如果在达到限制之前被发现。

#### 结果：
结果字段将是交易签名信息的数组，按从新到旧的顺序排列：
* `<object>`
  * `signature: <string>` - 交易签名为以base-58编码的字符串
  * `slot: <u64>` - 包含交易区块的插槽
  * `err: <object | null>` - 如果事务失败，则返回错误；如果事务成功，则返回null。 [TransactionError定义](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  * `memo: <string |null>` - 与交易关联的备忘录，如果没有备忘录，则为null

#### 示例:
请求：
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

结果：
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

返回已确认交易的交易详细信息

#### 参数：

- `<string>` - 交易签名，以base-58编码的字符串，N编码尝试使用特定于程序的指令解析器来返回`transaction.message.instructions`列表中更多的人性化和显式的数据。 如果请求“jsonParsed”但找不到解析器，则该指令将退回到常规JSON编码(`帐户`，`数据`和`programIdIndex`字段)。
- `<string>` -(可选) 用于返回的事务的编码，可以是“ json”，“ jsonParsed”，“ base58”(* slow *)或“ base64”。 如果未提供参数，则默认编码为JSON。

#### 结果：

- `<null>` - 如果未找到交易或未确认
- `<object>` - 如果区块得到确认，则具有以下字段的对象：
  - `slot: <u64>` - 处理该交易记录的插槽
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) 对象，采用JSON格式或已编码的二进制数据，具体取决于编码参数
  - `meta: <object | null>` - 交易状态元数据对象：
    - `err: <object | null>` - 如果交易失败，则返回错误；如果交易成功，则返回null。 [TransactionError定义](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - 此交易收取的费用，以u64整数表示
    - `preBalances: <array>` - 处理交易之前的u64帐户余额数组
    - `postBalances: <array>` - 处理交易后的u64帐户余额数组
    - `innerInstructions: <array|undefined>` -[内部指令](#inner-instructions-structure)列表，如果在此交易处理期间尚未启用内部指令记录，则将其省略
    - `logMessages: <array>` - 字符串日志消息的数组；如果在此交易期间尚未启用日志消息记录，则将其省略
    - DEPRECATED: `status: <object>` - 交易状态
      - `"Ok": <null>` - 交易成功
      - `"Err": <ERR>` - 交易失败，出现TransactionError

#### 示例:
请求：
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

结果：
```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [
        499998932500,
        26858640,
        1,
        1,
        1
      ],
      "preBalances": [
        499998937500,
        26858640,
        1,
        1,
        1
      ],
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
            "accounts": [
              1,
              2,
              3,
              0
            ],
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

#### 示例:
请求：
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

结果：
```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [
        499998932500,
        26858640,
        1,
        1,
        1
      ],
      "preBalances": [
        499998937500,
        26858640,
        1,
        1,
        1
      ],
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

返回当前epoch的信息

#### 参数：

- `<object>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果字段具有以下字段的对象：

- `absoluteSlot: <u64>`，当前插槽
- `块高度： <u64>`，当前区块高度
- `epoch: <u64>`，目前的epoch
- `slotIndex: <u64>`，相对于当前epoch开始的插槽
- `slotsInEpoch: <u64>`，此epoch的插槽数

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

结果：
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

从该集群的创世配置返回epoch时间表信息

#### 参数：

无

#### 结果：

结果字段具有以下字段的对象：

- `slotsPerEpoch: <u64>`，每个时期的最大插槽数
- `leaderScheduleSlotOffset：<u64>`，在某个时期开始之前的槽数，以计算该时期的领导者时间表
- `warmup：<bool>`，epoch是否开始短而长
- `firstNormalEpoch：<u64>`，第一个正常长度的时期，log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot：<u64>`，MINIMUM_SLOTS_PER_EPOCH \ *(2.pow(firstNormalEpoch)-1)

#### 例子：

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
'
```

结果：
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

### 获取费用计算器对于Blockhash

返回与查询区块哈希关联的费用计算器，如果区块哈希已过期，则返回`null`

#### 参数：

- `<string>` - 查询 blockhash 作为 Base58 编码的字符串
- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果将是一个 RpcResponse JSON 对象，其 `value` 等于：

- `<null>` - 如果查询 blockhash 已过期
- `<object>` - 否则为 JSON 对象，其中包含：
  - `feeCalculator：<object>`，`FeeCalculator`对象描述了在查询的区块哈希中的集群费率

#### 例子：

请求：
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

结果：
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

### getFeeRateGovernor

从根银行返回费率调控者信息

#### 参数：

没有

#### 结果：

` 结果`字段将是具有以下字段的`对象`：

- `burnPercent：<u8>`，收取的销毁费用百分比
- `maxLamportsPerSignature：<u64>`，` lamportsPerSignature` 可以获取最大值
- `minLamportsPerSignature: <u64>`，`lamportsPerSignature` 可以达到最小值
- `targetLamportsPerSignature：<u64>`，集群所需的费率
- `targetSignaturesPerSlot：<u64>`，集群所需的签名率

#### 例子：

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFeeRateGovernor"}
'
```

结果：
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

### getFees

从账本中返回最近的区块哈希值，可以用来计算使用该账目提交交易成本的费用明细表，以及该区块哈希值将在其中有效的最后一个时隙。

#### 参数：

- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果将是RpcResponse JSON对象，其中`value`设置为具有以下字段的JSON对象：

- `blockhash：<string>` - 以 base-58 编码的 Hash 字符串
- `feeCalculator：<object>` - FeeCalculator 对象，此区块哈希的费用明细表
- `lastValidSlot：<u64>` - 区块哈希有效的最后一个插槽

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

结果：
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

返回尚未从账本清除的最低确认区块的插槽

#### 参数：

无

#### 结果：

- `<u64>` - 插槽

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstAvailableBlock"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":250000,"id":1}
```

### getGenesisHash

返回起源哈希值

#### 参数：

无

#### 结果：

- `<string>` - 哈希值以 base-58 编码的字符串

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":"GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC","id":1}
```

### getHealth

返回节点的当前运行状况。

如果将一个或多个 `--trusted-validator` 参数提供给 `solana-validator`，则当节点位于最高可信验证器的 `HEALTH_CHECK_SLOT_DISTANCE` 插槽内时，将返回 "ok"，否则将返回错误。  如果未提供受信任的验证器，则始终返回“ ok”。

#### 参数：

无

#### 结果：

如果该节点运行状况良好，则为 "ok"；如果该节点运行状况不良，则将返回 JSON RPC 错误响应。  错误响应的详细信息是 **不稳定**，将来可能会更改


#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

健康结果：
```json
{"jsonrpc":"2.0","result": "ok","id":1}
```

不健康的结果(通用)：
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

不健康的结果(如果有其他信息可用)
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

返回当前节点的身份发布密钥

#### 参数：

无

#### 结果：

结果字段将是具有以下字段的 JSON 对象：

- `identity`，当前节点的身份发布密钥(以 base-58 编码的字符串)

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":{"identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN"},"id":1}
```

### getInflationGovernor

返回当前的通胀调控器

#### 参数：

- `<object>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果字段将是具有以下字段的JSON对象：

- `initial：<f64>`，从时间 0 开始的初始通胀百分比
- `terminal：<f64>`，终端通胀百分比
- `taper：<f64>`，每年降低通货膨胀率
- `foundation：<f64>`，分配给基金会的总通货膨胀百分比
- `foundationTerm：<f64>`，基础池通货膨胀的持续时间，以年为单位

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
'
```

结果：
```json
No translations matched your search
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

返回当前epoch的特定通货膨胀值

#### 参数：

无

#### 结果：

结果字段将是具有以下字段的JSON对象：

- `total：<f64>`，总通货膨胀
- `validator：<f64>`，分配给验证节点的通货膨胀
- `foundation：<f64>`，将通货膨胀分配给基金会
- `epoch：<f64>`，这些值对其有效的时代

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":{"epoch":100,"foundation":0.001,"total":0.149,"validator":0.148},"id":1}
```

### getLargestAccounts

按Lamport余额返回20个最大帐户

#### 参数：

- `<object>` -(可选) 包含以下可选字段的配置对象：
  - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)
  - (可选)`filter：<string>` - 按帐户类型过滤结果；当前支持：`circulating | nonCirculating`

#### 结果：

结果将是一个RpcResponse JSON对象，其`值`等于一个数组：

- `<object>` - 否则为JSON对象，其中包含：
  - `address：<string>`，以 Base-58 为底的帐户编码地址
  - `lamports：<u64>`，帐户中 lamport 的数量，以 u64 表示

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

结果：
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

获取领导者时间表

#### 参数：

- `<u64>` -(可选) 获取与提供的插槽相对应的时代的领导者时间表。 如果未指定，则获取当前时期的领导者时间表
- `<object>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `<null>` - 如果未找到请求的 epoch
- `<object>` - 否则，结果字段将是领导者公钥(以 Base-58 编码的字符串) 及其对应的领导者插槽索引作为值的字典(索引相对于所请求 epoch 中的第一个插槽)

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}
'
```

结果：
```json
{
  "jsonrpc":"2.0",
  "result":{
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63]
  },
  "id":1
}
```

### getMinimumBalanceForRentExemption

返回免除帐户租金所需的最低余额。

#### 参数：

- `<usize>` - 帐户数据长度
- `<object>` - (可选的) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `<u64>` - 帐户中所需的最低 lamport

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}
'
```

结果：
```json
{"jsonrpc":"2.0","result":500,"id":1}
```

### getMultipleAccounts

返回帐户信息以获取公钥列表

#### 参数：

- `<array>` - 要查询的公钥数组，以 base-58 编码的字符串
- `<object>` -(可选) 包含以下可选字段的配置对象：
  - (可选)[承诺](jsonrpc-api.md#configuring-state-commitment)
  - `encoding：<string>` - 帐户数据的编码，可以是 "base58"(*慢一点*)，"base64"，"base64+zstd" 或 "jsonParsed"。 "base58" 仅限于少于 128 个字节的帐户数据。 "base64" 将为任何大小的帐户数据返回 base64 编码的数据。 "base64+zstd" 使用 [Zstandard](https://facebook.github.io/zstd/) 压缩帐户数据，并对结果进行 base64 编码。 "jsonParsed" 编码尝试使用特定于程序的状态解析器来返回更多的人类可读和显式的帐户状态数据。 如果请求了"jsonParsed"，但找不到解析器，则该字段回退为 "base64" 编码，当 `data` 字段为 `<string>` 类型时可以检测到。
  - (可选) `dataSlice：<object>` - 使用提供的 `offset：<usize>` 和 `length：<usize>`字段限制返回的帐户数据；仅适用于 "base58"，"base64" 或 "base64+zstd" 编码。


#### 结果：

结果将是一个RpcResponse JSON对象，其`值`等于：

数组：

- `<null>` - 如果该 Pubkey 上的帐户不存在
- `<object>` - 否则为 JSON 对象，其中包含：
  - `lamports：<u64>`，分配给此帐户的 lamport 数量，以 u64 表示
  - `owner：<string>`，此帐户已分配给该程序的 base-58 编码的 Pubkey
  - `data：<[string，encoding] | object>`，与帐户关联的数据，可以是编码的二进制数据，也可以是 JSON 格式的 `{<program>: <state>}`，具体取决于编码参数
  - `executable：<bool>`，布尔值，指示帐户是否包含程序(并且严格为只读)
  - `rentEpoch：<u64>`，此帐户下一次将要欠租金的时期，即 u64

#### 示例:

请求：
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

结果：
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
          "AAAAAAEAAAACtzNsyJrW0g==",
          "base64"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": [
          "",
          "base64"
        ],
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

#### 示例:
请求：
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

结果：
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
        "data": [
          "",
          "base58"
        ],
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

返回提供的程序公钥拥有的所有帐户

#### 参数：

- `<string>` - 程序的发布密钥，以 base-58 编码的字符串
- `<object>` -(可选) 包含以下可选字段的配置对象：
  - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)
  - `encoding<string>` - 帐户数据的编码，可以是 "base58"(*slow*)，"base64"，"base64+zstd" 或 "jsonParsed"。 "base58" 仅限于少于 128 个字节的帐户数据。 "base64" 将为任何大小的帐户数据返回 base64 编码的数据。 "base64+zstd" 使用 [Zstandard](https://facebook.github.io/zstd/) 压缩帐户数据，并对结果进行 base64 编码。 "jsonParsed" 编码尝试使用特定于程序的状态解析器来返回更多的人类可读和显式的帐户状态数据。 如果请求了"jsonParsed"，但找不到解析器，则该字段回退为 "base64" 编码，当 `data` 字段为 `<string>` 类型时可以检测到。
  - (可选)`dataSlice：<object>` - 使用提供的 `offset: <usize>` 和 `length: <usize>` 字段限制返回的帐户数据；仅适用于 "base58"，"base64" 或 "base64+zstd" 编码。
  - (可选)`filters：<array>` - 使用各种 [filter objects](jsonrpc-api.md#filters) 过滤结果；帐户必须满足所有过滤条件，才能包含在结果中

##### 过滤器：
- `memcmp：<object>` - 比较提供的一系列字节和程序帐户数据的特定偏移量。 栏位：
  - `offset：<usize>` - 进入计划帐户数据的偏移量以开始比较
  - `bytes：<string>` - 要匹配的数据，以 base-58 编码的字符串

- `dataSize：<u64>` - 比较程序帐户数据长度与提供的数据大小

#### 结果：

结果字段将是一个JSON对象数组，其中包含：

- `pubkey：<string>` - 帐户 Pubkey 作为以 Base-58 为底的编码字符串
- `account：<object>` - 一个 JSON 对象，具有以下子字段：
   - `lamports：<u64>`，分配给此帐户的 lamport 数量，以 u64 表示
   - `owner：<string>`，此帐户已分配给程序的 base-58 编码程序的 Pubkey `data：<[string，encoding] | object>`，与帐户相关联的数据，可以是编码的二进制数据或 JSON 格式为 `{<program>: <state>}`，具体取决于编码参数
   - `executable：<bool>`，布尔值，指示帐户是否包含程序(并且严格为只读)
   - `rentEpoch：<u64>`，此帐户下一次将要欠租金的时期，即 u64

#### 示例:
请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}
'
```

结果：
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

#### 示例:
请求：
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

结果：
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

从账本返回最近的区块哈希值，以及可用于计算使用该账目提交交易的费用的费用表。

#### 参数：

- `<object>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

一个 RPC 响应，其中包含一个由字符串 blockhash 和 FeeCalculator JSON 对象组成的JSON 对象。

- `RpcResponse <object>` - RpcResponse JSON 对象，其中 `value` 字段设置为 JSON 对象，包括：
- `blockhash：<string>` - 以 base-58 编码的 Hash 字符串
- `feeCalculator：<object>` - FeeCalculator 对象，此区块哈希的费用明细表

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d 'i
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
'
```

结果：
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

以相反的插槽顺序返回最近的性能样本列表。 每60秒进行一次性能采样，其中包括在给定时间窗口内发生的交易和插槽的数量。

#### 参数：
- `limit：<usize>` -(可选) 要返回的样本数(最大为 720)

#### 结果：

数组：

- `RpcPerfSample<object>`
  - `slot：<u64>` - 在以下位置采样的插槽
  - `numTransactions：<u64>` - 样本中的交易数量
  - `numSlots：<u64>` - 样本中的插槽数
  - `samplePeriodSecs：<u16>` - 示例窗口中的秒数

#### 示例:

请求：
```bash
// Request
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getRecentPerformanceSamples", "params": [4]}
'
```

结果：
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

返回节点具有快照的最高插槽

#### 参数：

无

#### 结果：

- `<u64>` - Snapshot slot

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSnapshotSlot"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":100,"id":1}
```

节点没有快照时的结果：
```json
{"jsonrpc":"2.0","error":{"code":-32008,"message":"No snapshot"},"id":1}
```

### getSignatureStatuses

返回签名列表的状态。 除非包括`searchTransactionHistory`配置参数，否则此方法仅搜索签名的最新状态缓存，该缓存会保留所有活动插槽以及` MAX_RECENT_BLOCKHASHES`根目录插槽的状态。

#### 参数：

- `<array>` - 确认交易签名的数组，以 base-58 编码的字符串
- `<object>` -(可选) 包含以下字段的配置对象：
  - `searchTransactionHistory：<bool>` - 如果为 true，Solana 节点将在其账本缓存中搜索在最近状态缓存中未找到的任何签名

#### 结果：

一个 RpcResponse，其中包含一个由 TransactionStatus 对象数组组成的 JSON 对象。

- `RpcResponse <object>` - 具有 `value` 字段的 RpcResponse JSON 对象：

数组：

- `<null>` - 未知交易
- `<object>`
  - `slot：<u64>` - 交易处理的插槽
  - ` confirmations：<usize | null>` - 自签名确认以来的块数，如果已植根则为 null，并由集群的绝大多数决定
  - `err：<object | null>` - 如果交易失败，则返回错误；如果交易成功，则返回 null。 [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus：<string | null>` - 交易的集群确认状态； `processed`，` confirmed` 或 `finalized`。 有关乐观确认的更多信息，请参见 [承诺](jsonrpc-api.md#configuring-state-commitment)。
  - 弃用：` status：<object>` - 交易状态
    - `"Ok"：<null>` - 交易成功
    - `"Err"：<ERR>` - 交易失败，出现 TransactionError

#### 示例:

请求：
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

结果：
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
        "confirmationStatus": "confirmed",
      },
      null
    ]
  },
  "id": 1
}
```

#### 示例:
请求：
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

结果：
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
        "confirmationStatus": "finalized",
      },
      null
    ]
  },
  "id": 1
}
```

### getSlot

返回节点正在处理的当前插槽

#### 参数：

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `<u64>` - 当前插槽

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":1234,"id":1}
```

### getSlotLeader

返回当前的插槽领导

#### 参数：

- `<object>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `<string>` - 节点身份 Pubkey 作为 base-58 编码的字符串

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":"ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS","id":1}
```

### getStakeActivation

返回权益账户的epoch激活信息

#### 参数：

* `<string>` - 要查询的股份账户的公钥，以 base-58 编码的字符串
* `<object>` - (可选) 包含以下可选字段的配置对象：
  * (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)
  * (可选)`epoch：<u64>` - 用于计算激活详细信息的时期。 如果未提供参数，则默认为当前 epoch。

#### 结果：

结果将是具有以下字段的JSON对象：

* `state：<string` - 股份账户的激活状态，其中之一：`active`，`inactive`，`activating`，`deactivating`
* `active：<u64>` - 时期有效的股份
* `inactive：<u64>` - 在新时期无效的股份

#### 示例:
请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

结果：
```json
{"jsonrpc":"2.0","result":{"active":197717120,"inactive":0,"state":"active"},"id":1}
```

#### 示例:
请求：
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

结果：
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

返回有关当前电源的信息。

#### 参数：

- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果将是RpcResponse JSON对象，其`值`等于包含以下内容的JSON对象：

- `total：<u64>` - Lamports 的总供应量
- ` circulating：<u64>` - 以 lamports 的循环供应
- ` nonCirculating：<u64>` - 以 lamports 的的非循环供应
- `nonCirculatingAccounts：<array>` - 非流通账户的账户地址数组，作为字符串

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getSupply"}
'
```

结果：
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

返回SPL令牌帐户的代币余额。 **UNSTABLE**

#### 参数：

- `<string>` - 要查询的代币帐户的公钥，以 base-58 编码的字符串
- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果将是RpcResponse JSON对象，其`值`等于包含以下内容的JSON对象：

- `uiAmount：<f64>` - 余额，使用薄荷规定的小数
- `amount：<string>` - 不带小数的原始余额，u64 的字符串表示形式
- `decimals：<u8>` - 小数点右边的以 10 为基数的数字

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStEuGbc3sM84cKRL6yYaaSstyLSU4ve5oovLS7"]}
'
```

结果：
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

通过批准的代表返回所有SPL令牌帐户。 **UNSTABLE**

#### 参数：

- `<string>` - 要查询的帐户委托人的公钥，以 base-58 编码的字符串
- `<object>` - 可以：
  * `mint：<string>` - 特定令牌Mint的发布密钥，用于将帐户限制为以 base-58 编码的字符串；或者
  * `programId：<string>` - 拥有帐户的Token程序 ID 的 Pubkey，以 base-58 编码的字符串
- `<object>` - (可选) 包含以下可选字段的配置对象：
  - (可选)[承诺](jsonrpc-api.md#configuring-state-commitment)
  - `encoding：<string>` - 帐户数据的编码，可以是 "base58"(*慢一点*)，"base64"，"base64+zstd" 或 "jsonParsed"。 "jsonParsed" 编码尝试使用特定于程序的状态解析器来返回更多的人类可读和显式的帐户状态数据。 如果请求 "jsonParsed"，但找不到特定帐户的有效铸造，则该帐户将从结果中滤除。
  - (可选) `dataSlice：<object>` - 使用提供的 `offset: <usize>` 和 `length: <usize>`字段限制返回的帐户数据；仅适用于 "base58"，"base64" 或 "base64+zstd" 编码。

#### 结果：

结果将是RpcResponse JSON对象，其`值`等于JSON对象的数组，其中将包含：

- `pubkey：<string>` - 帐户 Pubkey 作为以 Base-58 为底的编码字符串
- `account：<object>` - 一个JSON对象，具有以下子字段：
   - `lamports：<u64>`，分配给此帐户的 lamport 数量，以 u64 表示
   - `owner：<string>`，此帐户已分配给该程序的 base-58 编码的 Pubkey
   - `data：<object>`，与帐户关联的令牌状态数据，可以是编码的二进制数据，也可以是JSON 格式的 `{<program>: <state>}`
   - `executable: <bool>`，布尔值，指示帐户是否包含程序 \(并且严格为只读\)
   - ` rentEpoch：<u64>`，此帐户下一次将要欠租金的时期，即 u64

#### 示例:

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

结果：
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

按代币所有者返回所有SPL代币帐户。 **UNSTABLE**

#### 参数：

- `<string>` - 要查询的帐户所有者的公钥，以 base-58 编码的字符串
- `<object>` - 可以：
  * `mint：<string>` - 特定代币 Mint 的发布密钥，用于将帐户限制为以 base-58 编码的字符串；或者
  * `programId：<string>` - 拥有帐户的 Token 程序ID的 Pubkey，以base-58编码的字符串
- `<object>` - (可选) 包含以下可选字段的配置对象：
  - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - 帐户数据的编码，可以是 "base58"(*slow*)，"base64"，"base64+zstd" 或 "jsonParsed"。 "jsonParsed" 编码尝试使用特定于程序的状态解析器来返回更多的人类可读和显式的帐户状态数据。 如果请求 "jsonParsed"，但找不到特定帐户的有效铸造，则该帐户将从结果中滤除。
  - (可选) `dataSlice：<object>` - 使用提供的 `offset：<usize>` 和 `length：<usize>`字段限制返回的帐户数据；仅适用于 "base58"，"base64" 或 "base64+zstd" 编码。

#### 结果：

结果将是RpcResponse JSON对象，其`值`等于JSON对象的数组，其中将包含：

- `pubkey：<string>` - 帐户 Pubkey 作为以 Base-58 为底的编码字符串
- `account：<object>` - 一个JSON 对象，具有以下子字段：
   - `lamports：<u64>`，分配给此帐户的 lamport 数量，以 u64 表示
   - `owner：<string>`，此帐户已分配给该程序的 base-58 编码的 Pubkey
   - `data：<object>`，与帐户关联的令牌状态数据，可以是编码的二进制数据，也可以是JSON 格式的 `{<program>: <state>}`
   - `executable: <bool>`，布尔值，指示帐户是否包含程序\(并且严格为只读\)
   - `rentEpoch：<u64>`，此帐户下一次将要欠租金的时期，即 u64

#### 示例:

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

结果：
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

返回特殊的 SPL Token 类型的 20 个最大账户。 **UNSTABLE**

#### 参数：

- `<string>` - 要查询的代币 Mint 的公钥，以 base-58 编码的字符串
- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果将是RpcResponse JSON对象，其`值`等于包含以下内容的JSON对象数组：

- `address：<string>` - 代币帐户的地址
- `uiAmount：<f64>` - 代币账户余额，使用薄荷规定的小数
- `amount：<string>` - 不带小数的原始令牌帐户余额，是 u64 的字符串表示形式
- `decimals：<u8>` - 小数点右边的以 10 为基数的数字

#### 示例：

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

结果：
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

返回SPL代币类型的总供给。 **UNSTABLE**

#### 参数：

- `<string>` - 要查询的代币 Mint 的公钥，以 base-58 编码的字符串
- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果将是RpcResponse JSON对象，其`值`等于包含以下内容的JSON对象：

- `uiAmount：<f64>` - 代币总供给，使用薄荷规定的小数
- `amount：<string>` - 不带小数的原始令牌总数，u64 的字符串表示形式
- `decimals：<u8>` - 小数点右边的以 10 为基数的数字

#### 示例：

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

结果：
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

从账本中返回当前交易计数

#### 参数：

- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `<u64>` - 计数

#### 示例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

结果：
```json
{"jsonrpc":"2.0","result":268,"id":1}
```

### getVersion

返回在节点上运行的当前solana版本

#### 参数：

无

#### 结果：

结果字段将是具有以下字段的JSON对象：

- `solana-core`，solana-core 的软件版本
- `feature-set`，当前软件功能集的唯一标识符

#### 示例:

请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

结果：
```json
{"jsonrpc":"2.0","result":{"solana-core": "1.6.0"},"id":1}
```

### getVoteAccounts

返回当前银行中所有有投票权的帐户的帐户信息和相关权益。

#### 参数：

- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

结果字段将是 `current` 帐户和 `delinquent` 帐户的JSON对象，每个帐户都包含带有以下子字段的JSON对象数组：

- `votePubkey：<string>` - 投票帐户公钥，以base-58编码的字符串
- `nodePubkey：<string>` - 节点公钥，以base-58编码的字符串
- `activatedStake：<u64>` - 抽奖活动中的股份，委托给该投票帐户，并在该时期处于活动状态
- `epochVoteAccount：<bool>` - 布尔，是否为此时代投注了投票帐户
- `commission: <number>`，应支付给投票帐户的奖励支出的百分比(0-100)
- `lastVote：<u64>` - 此投票帐户最近投票过的广告位
- `epochCredits：<array>` - 每个时期结束时获得多少学分的历史，以包含 `[epoch，credits，previousCredits]` 的阵列数组的形式出现

#### 示例:
请求：
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

结果：
```json
{
  "jsonrpc": "2.0",
  "result": {
    "current": [
      {
        "commission": 0,
        "epochVoteAccount": true,
        "epochCredits": [
          [ 1, 64, 0 ],
          [ 2, 192, 64 ]
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

返回节点在其账本中具有有关信息的最低插槽。 如果将节点配置为清除较早的账本数据，则此值可能会随着时间增加

#### 参数：

无

#### 结果：

- `u64` - 最小账本插槽

#### 示例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

结果：
```json
{"jsonrpc":"2.0","result":1234,"id":1}
```

### requestAirdrop

向空头请求空投空投

#### 参数：

- `<string>` - 帐户的公钥，用于接收 lamport，以 base-58 编码的字符串
- `<integer>` - lamports，如 u64
- `<object>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment) (用于检索区块哈希和验证空投成功)

#### 结果：

- `<string>` - 空投的交易签名，以base-58编码的字符串

#### 示例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}
'

```

结果：
```json
{"jsonrpc":"2.0","result":"5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW","id":1}
```

### sendTransaction

将已签名的交易提交到集群以进行处理。

此方法不会以任何方式更改交易；它将客户端创建的交易按原样中继到节点。

如果节点的rpc服务接收到该交易，则此方法将立即成功，而无需等待任何确认。 此方法的成功响应不能保证集群能够处理或确认交易。

尽管rpc服务将合理地重试提交，但是如果交易的 `recent_blockhash` 在到达之前到期，则该交易可能会被拒绝。

使用 [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) 确保交易得到处理和确认。

提交之前，请执行以下飞行前检查：

1. 交易签名已验证
2. 根据预检承诺指定的银行插槽模拟交易。 失败时将返回错误。 如果需要，可以禁用预检检查。 建议指定相同的承诺和飞行前承诺，以避免混淆行为。

返回的签名是交易中的第一个签名，用于标识交易([交易ID](../../terminology.md#transanction-id))。 在提交之前，可以很容易地从交易数据中提取该标识符。

#### 参数：

- `<string>` - 完全签名的交易，作为编码的字符串
- `<object>` -(可选) 包含以下字段的配置对象：
  - `skipPreflight：<bool>` - 如果为 true，则跳过预检交易检查(默认值：false)
  - `preflightCommitment：<string>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment) 用于预检的级别(默认值：`"max"`)。
  - `encoding：<string>` -(可选)用于交易数据的编码。 `"base58"`(*slow*，**DEPRECATED**) 或 `"base64"`。 (默认值：`"base58"`)。

#### 结果：

- `<string>` - 嵌入在交易中的第一个交易签名，以 base-58 编码的字符串([transaction id](../../terminology.md#transanction-id))

#### 示例:

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

结果：
```json
{"jsonrpc":"2.0","result":"2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb","id":1}
```

### simulateTransaction

模拟发送交易

#### 参数：

- `<string>` - 交易，作为编码的字符串。 该交易必须具有有效的散列，但不要求对其进行签名。
- `<object>` -(可选) 包含以下字段的配置对象：
  - `sigVerify：<bool>` - 如果为 true，则将验证交易签名(默认值：false)
  - `commitment：<string>` - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment) 级别以(默认值：`"max"`) 模拟交易。
  - `encoding：<string>` - (可选) 用于交易数据的编码。 `"base58"` (*slow*，**DEPRECATED**) 或 `"base64"`。 (默认值：`"base58"`)。

#### 结果：

包含TransactionStatus对象的RpcResponse结果将是RpcResponse JSON对象，其中`value`设置为具有以下字段的JSON对象：

- `err：<object>` - 如果交易失败，则返回错误；如果交易成功，则返回null。 [TransactionError定义](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs：<array | null>` - 交易指令在执行期间输出的日志消息数组，如果在交易能够执行之前模拟失败(例如，由于无效的哈希或签名验证失败)，则返回 null

#### 示例:

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

### setLogFilter

在验证器上设置日志过滤器

#### 参数：

- `<string>` - 要使用的新日志过滤器

#### 结果：

- `<null>`

#### 示例:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"setLogFilter", "params":["solana_core=debug"]}
'
```

结果：
```json
{"jsonrpc":"2.0","result":null,"id":1}
```

### validatorExit

如果验证器在启用RPC退出的情况下启动(`--enable-rpc-exit`参数)，则此请求将导致验证器退出。

#### 参数：

无

#### 结果：

- `<bool>` - 验证程序退出操作是否成功

#### 示例：:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"validatorExit"}
'

```

结果：
```json
{"jsonrpc":"2.0","result":true,"id":1}
```

## 订阅Websocket

在 `ws://<ADDRESS>/` 连接到RPC PubSub Websocket之后：

- 使用以下方法向Websocket提交订阅请求
- 多个订阅可能一次处于活动状态
- 许多订阅都采用可选的[`commitment` 参数](jsonrpc-api.md#configuring-state-commitment)，定义了如何最终完成更改以触发通知。 对于订阅，如果未指定承诺，则默认值为 `"singleGossip"`。

### accountSubscribe

订阅一个帐户以在给定帐户公钥的灯饰或数据发生更改时接收通知

#### 参数：

- `<string>` - 帐户 Pubkey，以 base-58 编码的字符串
- `<object>` - (可选) 包含以下可选字段的配置对象：
  - `<object>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment)
  - `encoding：<string>` - 帐户数据的编码，可以是 "base58"(*slow*)，"base64"，"base64+zstd" 或 "jsonParsed"。 "jsonParsed" 编码尝试使用特定于程序的状态解析器来返回更多的人类可读和显式的帐户状态数据。 如果请求 "jsonParsed" 但找不到解析器，则该字段将退回到二进制编码，当 `data` 字段为 `<string>` 类型时可检测到。

#### 结果：

- `<number>` - Subscription id \(needed to unsubscribe\)

#### 示例:

请求：
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

Result:
```json
{"jsonrpc": "2.0","result": 23784,"id": 1}
```

#### 通知形式

Base58 编码：
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
        "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
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

Parsed-JSON编码：
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
        "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
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

取消订阅帐户更改通知

#### 参数：

- `<number>` - 要取消的帐户订阅ID

#### 结果：

- `<bool>` - 取消订阅成功消息

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"accountUnsubscribe", "params":[0]}

```

结果：
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### logsSubscribe

订阅交易日志记录。  **UNSTABLE**

#### 参数：

- `filter：<string><object>` - 过滤条件，日志按帐户类型接收结果；目前支持：
  - "all" - 订阅除简单投票交易以外的所有交易
  - "allWithVotes" - 订阅所有交易，包括简单的投票交易
  - `{ "mentions": [ <string> ] }` - 订阅所有提及提供的Pubkey的交易(以 base-58 编码的字符串)
- `<object>` -(可选) 包含以下可选字段的配置对象：
  - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### 示例:

请求：
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

结果：
```json
{"jsonrpc": "2.0","result": 24040,"id": 1}
```

#### 通知形式

Base58 编码：
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

退订交易日志

#### 参数：

- `<integer>` - 要取消的订阅 id

#### 结果：

- `<bool>` - 取消订阅成功消息

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"logsUnsubscribe", "params":[0]}

```

结果：
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### programSubscribe

订阅程序以在程序拥有的给定帐户的灯饰或数据发生更改时接收通知

#### 参数：

- `<string>` - program_id Pubkey，以 base-58 编码的字符串
- `<object>` -(可选) 包含以下可选字段的配置对象：
  - (可选) [承诺](jsonrpc-api.md#configuring-state-commitment)
  - `encoding：<string>` - 帐户数据的编码，可以是“ base58”(*slow*)，“ base64”，“ base64 + zstd”或“ jsonParsed”。 “jsonParsed”编码尝试使用特定于程序的状态解析器来返回更多的人类可读和显式的帐户状态数据。 如果请求“ jsonParsed”但找不到解析器，则该字段将回退为base64编码，当 `data` 字段为 `<string>` 类型时可检测到。
  - (可选) `filters：<array>` - 使用各种 [filter objects](jsonrpc-api.md#filters) 过滤结果；帐户必须满足所有过滤条件，才能包含在结果中

#### 结果：

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### 示例:

请求：
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

结果：
```json
{"jsonrpc": "2.0","result": 24040,"id": 1}
```

#### 通知形式

Base58 编码：
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
          "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        },
      }
    },
    "subscription": 24040
  }
}
```

Parsed-JSON 编码：
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
          "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        },
      }
    },
    "subscription": 24040
  }
}
```

### programUnsubscribe

退订计划拥有的帐户更改通知

#### 参数：

- `<integer>` - 要取消的帐户订阅 Id

#### 结果：

- `<bool>` - 取消订阅成功消息

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"programUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### signatureSubscribe

订阅交易签名以在交易确认后接收通知。在 `signatureNotification` 上，订阅将自动取消。

#### 参数：

- `<string>` - 交易签名，以 base-58 编码的字符串
- `<object>` -(可选) [承诺](jsonrpc-api.md#configuring-state-commitment)

#### 结果：

- `integer` - 订阅 id(需要取消订阅)

#### 示例:

请求：
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

Result:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### 通知形式
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

退订签名确认通知

#### 参数：

- `<integer>` - 要取消的订阅 Id

#### 结果：

- `<bool>` - 取消订阅成功消息

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"signatureUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### slotSubscribe

订阅者可在验证节点处理插槽后随时接收通知

#### 参数：

无

#### 结果：

- `integer` - 订阅id(需要取消订阅)

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"slotSubscribe"}

```

Result:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### 通知形式

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

取消订阅槽通知

#### 参数：

- `<integer>` - 需要取消的订阅 id

#### 结果：

- `<bool>` - 取消订阅成功消息

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"slotUnsubscribe", "params":[0]}

```

结果：
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### rootSubscribe

只要验证节点设置了新的根目录，即可订阅以接收通知。

#### 参数：

无

#### 结果：

- `integer` - 订阅 id (需要取消订阅)

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"rootSubscribe"}

```

结果：
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### 通知形式

结果是最新的根插槽号

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

### 取消订阅

退订根通知

#### 参数：

- `<integer>` - 要取消的订阅ID

#### 结果：

- `<bool>` - 取消订阅成功消息

#### 示例：:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"rootUnsubscribe", "params":[0]}

```

结果：
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### 投票订阅-不稳定，默认情况下处于禁用状态

**此订阅是不稳定的，并且仅在验证器以`--rpc-pubsub-enable-vote-subscription`标志启动时可用。  此订阅的格式将来可能会更改**

订阅以在八卦中观察到新的投票时接收通知。 这些票是事先同意的，因此不能保证这些票会进入账本。

#### 参数：

无

#### 结果：

- `integer` - 订阅 id (需要取消订阅)

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"voteSubscribe"}

```

Result:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### 通知形式

结果是最新的投票，其中包含其哈希值，已投票的时隙列表以及可选的时间戳。

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

### 投票退订

退订投票通知

#### 参数：

- `<integer>` - 要取消的订阅ID

#### 结果：

- `<bool>` - 取消订阅成功消息

#### 示例:

请求：
```json
{"jsonrpc":"2.0", "id":1, "method":"voteUnsubscribe", "params":[0]}
```

响应：
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```
