---
title: JSON RPC API
---

솔라나 노드는 [JSON-RPC 2.0](https://www.jsonrpc.org/specification) 사양을 사용하여 HTTP 요청을 수락합니다.

JavaScript 애플리케이션 내에서 Solana 노드와 상호 작용하려면 RPC 메소드에 편리한 인터페이스를 제공하는 [solana-web3.js](https://github.com/solana-labs/solana-web3.js) 라이브러리를 사용하세요.

## RPC HTTP 엔드포인트

**Default port:** 8899 예. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## RPC PubSub WebSocket 엔드포인트

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## 메서드

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
- [Subscription Websocket](jsonrpc-api.md#subscription-websocket)
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

## 불안정한 메서드

불안정한 메서드들은 배포되는 패치마다 급격한 변화를 가질 수 있으며 영원히 지원되지 않을 수도 있습니다.

- [getTokenAccountBalance](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [getTokenLargestAccounts](jsonrpc-api.md#gettokenlargestaccounts)
- [getTokenSupply](jsonrpc-api.md#gettokensupply)

## 요청 형식

JSON-RPC 요청을하려면`Content-Type: application/json` 와 함께 HTTP POST 요청을 보냅니다. JSON 요청 데이터는 4개의 필드를 포함해야합니다.

- `jsonrpc: <string>`,`"2.0"`으로 설정
- `id: <number>`, 고유한 클라이언트 생성 식별 정수
- `method: <string>`, 호출할 메소드를 포함하는 문자열
- `params: <array>`, 정렬된 매개 변수 값의 JSON 배열

curl을 통한 예시:

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

결과 필드는 다음을 포함하는 JSON 객체입니다:

- `jsonrpc: <string>`, 요청 사항과 매칭
- number>`, 요청 식별자와 일치
- `result: <array|number|object|string>`, 성공적으로 요청된 데이터 또는 성공 확정

요청은 단일 POST에 대한 데이터로 JSON-RPC 요청 객체 배열을 전송하여 일괄적으로 전송할 수 있습니다.

## 정의

- 해시: 데이터 청크의 SHA-256 해시입니다.
- Pubkey: Ed25519 키 쌍의 공개키입니다.
- 트랜잭션: 작업을 승인하기 위해 클라이언트 키 쌍이 서명 한 솔라나 지침 목록입니다.
- 서명: 명령을 포함한 거래 페이로드 데이터의 Ed25519 서명. 이는 트랜잭션을 식별하는데 사용할 수 있습니다.

## State Commitment 구성

사전 확인 및 트랜잭션 처리를 위해 솔라나 노드는 클라이언트가 설정한 커밋 요구 사항에 따라 쿼리할 뱅크 상태를 선택합니다. 이 커밋은 해당 시점에서 블록이 어떻게 완성되었는지를 설명합니다. 원장 상태를 쿼리할 때 진행 상황을 보고하려면 더 낮은 수준의 커밋을 사용하고 상태가 롤백되지 않도록 하려면 더 높은 수준을 사용하는 것이 좋습니다.

커밋의 내림차순(가장 강한 완료에서 가장 약한 완료 순) 으로 클라이언트는 다음을 지정할 수 있습니다.

- `"max"` - 노드는 클러스터의 압도적 다수가 확정하고 최대의 락아웃에 도달한 가장 최근의 블록을 쿼리하며, 이는 클러스터가 해당 블록이 종결되었다고 취급한다는 의미입니다.
- `"root"`- 노드는 최대 락업에 도달한 가장 최근 블록을 쿼리합니다. 즉, 노드가 해당 블록을 종결되었다고 인식했다는 것을 의미합니다.
- `"singleGossip"`- 노드가 클러스터의 압도적 다수가 투표한 가장 최근 블록을 쿼리합니다.
  - 가십과 릴레이로부터 투표를 모읍니다.
  - 자식 블록에 대한 투표는 계산하지 않고 해당 블록에 대한 직접 투표만 계산합니다.
  - 확정 레벨은 "낙천적 확정"을 지지하며 1.3 릴리즈 이후부터 확정됩니다.
- `"recent"`- 노드는 가장 최근 블록을 쿼리합니다. 해당 블록은 완전하지 않을 수 있습니다.

많은 종속 트랜잭션을 연속적으로 처리하려면 속도와 롤백 안전성의 균형을 유지하는`"singleGossip"`커밋을 사용하는 것이 좋습니다. 안전을 위해`"max"`커밋을 사용하는 것이 좋습니다.

#### 예제 :

예는 커밋 매개 변수가`params` 배열의 마지막 요소로 포함되어야 합니다:

```bash
curl http : // localhost : 8899 -X POST -H "Content-Type : application / json "-d '
  {
    "jsonrpc ":"2.0 ",
    "id ": 1,
    "method ":"getBalance ",
    "params ": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri "
    ]
  }
'
```

#### 디폴트:

만일 커밋 설정이 주어지지 않는다면 노드는 기본적으로 `"max"` 커밋으로 맞춰집니다.

뱅크 상태를 쿼리하는 메서드만 커밋의 패리미터를 받습니다. 이는 아래의 API 레퍼런스를 참고합니다.

#### RpcResponse Structure

커밋 패리미터를 받는 많은 메서드는 두 부분으로 이뤄진 RpcResponse JSON 오브젝트를 반환합니다:

- `context` : 연산이 평가된 `slot` 을 포함한 RpcResponseContext JSON 구조.
- `value` : 연산으로 반환된 값.

## 건전성 확인

JSON RPC API는 아니지만, RPC HTTP 엔드포인트의 `GET /health`는 로드 밸런서나 여타 네트워크 인프라에 사용될 건전성 확인 메커니즘을 제공합니다. 요청은 항상 다음 조건에 따라 "ok"나 "behind" 내용의 HTTP 200 OK 응답을 반환합니다.

1. If one or more 만일 하나 이상의 `--trusted-validator` argument가 `solana-validator`에게 주어지면, "ok"가 반환됩니다. 만일 노드가 `HEALTH_CHECK_SLOT_DISTANCE` 최상위 밸리데이터 슬롯에 있다면, "behind"가 반환됩니다.
2. 신뢰할 수 있는 밸리데이터가 없을 경우엔 항상 "ok"가 반환됩니다.

## JSON RPC API 참조

### getAccountInfo

제공된 Pubkey의 계정 잔액을 반환합니다

#### Parameters:

- `<string>`- 쿼리 할 계정의 Pubkey, base-58 인코딩 문자열
- `<object>` - (조건부) 다음 조건부 필드를 담고있는 설정 오브젝트:
  - (조건부) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - "base58"나 (_slow_), "base64", "base64+zstd", 또는 "jsonParsed"로 암호화. "base58"는 128 바이트 미만의 계정 데이터로 제한됩니다. "base64"는 모든 사이즈의 계정 데이터에 대한 base64 암호화 데이터를 반환합니다. "base64+zstd"는 [Zstandard](https://facebook.github.io/zstd/)를 통해 계정 데이터를 압축하며 base64로 결과물을 암호화 합니다. "jsonParsed" 인코딩은 특정 프로그램에 대한 상태 파서를 사용하여 사람이 읽을 수 있고 표면적인 상태 데이터를 반환하도록 합니다. 만일 "jsonParsed"가 요청되었지만 파서를 찾을 수 없는 경우, 필드는 "base64" 인코등으로 돌아가며, `data`필드가 타이핑 되었을 때 `<string>`을 찾을 수 있게 됩니다.
  - (조건부) `dataSlice: <object>` - limit the returned account data using the 제공된 `offset: <usize>`과 `length: <usize>` 필드에 따라 반환되는 계정 데이터를 제한합니다; 오직 "base58", "base64" 또는 "base64+zstd" 인코딩에만 쓸 수 있습니다.

#### 결과:

결과는 `value`를 지닌 RpcResponse JSON 오브젝트 이며 다음과 같게 됩니다:

- `<null>` - 만일 요청된 계정이 존재하지 않을 시
- `<object>` - 또는, 다음을 내포하는 JSON 오브젝트:
  - `lamports: <u64>`, u64로 된 해당 계정에 할당된 램포트 수
  - `owner: <string>`, 해당 계정이 할당된프로그램의 base-58로 인코딩 된 Pubkey
  - `data: <[string, encoding]|object>`, 바이너리로 인코딩 된 데이터나 JSON 포멧으로 된 계정 관련 데이터 `{<program>: <state>}`, 인코딩 패리미터에 따라 다름.
  - `executable: <bool>`, 계정이 프로그램을 담고 있는 지에 대한 boolean \(읽기전용\)
  - `rentEpoch: <u64>`, u64로 표현되는 해당 계정이 다음 렌트를 가질 에폭

#### 예제:

요청:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62Y ypT7avPYvA"
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

응답:

```json
{
  "json rpc ":"2.0 ",
  "result ": {
    "context ": {
      "slot ": 1
    },
    "value ": {
      "data ": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKbcHRTPuR3oZ1EioK5vpe
        EioKtYGiYxcutable
      ],
      "
      " lamports ": 1000000000,
      "owner ":"11111111111111111111111111111111 ",
      "rentEpoch ": 2
    }
  },
  "id ": 1
}
```

#### 예제:

요청:

```bash
curl http : // localhost : 8899 -X POST -H "Content-Type : application / json"-d '
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

응답:

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

제공된 공개키 계정의 잔액을 반환합니다.

#### 패리미터:

- `<string>` - 쿼리할 계정의 공개키, base-58로 인코딩 된 스트링
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### 결과:

- `RpcResponse<u64>` - RpcResponse JSON object with `value` field set to the balance

#### 예제:

요청:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

결과:

```json
{
  "jsonrpc": "2.0",
  "result": { "context": { "slot": 1 }, "value": 0 },
  "id": 1
}
```

### getBlockCommitment

특정 블록에 대한 커밋을 반환합니다

#### 패리미터:

- `<u64>`- 슬롯으로 식별되는 블록

#### 결과:

결과 필드는 다음을 내포하는 JSON 오프젝트입니다:

- `commitment` - commitment, comprising either:
  - `<u64>`-슬롯식별되는 블록
  - `-알 수없는 블록`commitment`-commit,-`-`<array>`-commit, 클러스터 지분의 양을 기록하는 u64 정수 배열 0에서`MAX_LOCKOUT_HISTORY` + 1 -`totalStake`-현재 시대의 총 활성 지분
- `totalStake` - total active stake, in lamports, of the current epoch

#### Example:

Request:

```bash
curl http : / / localhost : 8899 -X POST -H "Content-Type : application / json"-d '
  { "jsonrpc": "2.0", "id": 1, "method": "getBlockCommitment", "params": [5 ]}
'
```

Result:

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

확인 된 블록의 예상 생산 시간을 반환합니다.

각 밸리데이터은 특정 블록에 대한 투표에 타임스탬프를 간헐적으로 추가하여 정기적으로 원장에 UTC 시간을보고합니다. 요청 된 블록의 시간은 원장에 기록 된 최근 블록 세트에있는 투표 타임스탬프의 지분 가중 평균에서 계산됩니다.

스냅 샷에서 부팅하거나 (오래된 슬롯을 제거하여) 원장 크기를 제한하는 노드는 가장 낮은 루트 +`TIMESTAMP_SLOT_RANGE` 아래의 블록에 대해 null 타임스탬프를 반환합니다. 이 기록 데이터에 관심이있는 사용자는 제네시스에서 구축되고 전체 원장을 유지하는 노드를 쿼리해야합니다.

#### Parameters:

- `<u64>`-u64 정수로

#### Results:

- `<i64>`-예상 생산 시간, Unix 타임스탬프 (Unix 시대 이후 초) \*`<null>`-이 블록에 대해 타임스탬프를 사용할 수 없습니다.
- `<null>` - timestamp is not available for this block

#### Example:

Request:

```bash
curl http : // localhost : 8899 -X POST -H "Content-Type : application / json"-d '
  { "jsonrpc" : "2.0", "id": 1, "method": "getBlockTime", "params": [5]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### getClusterNodes

클러스터에 참여하는 모든 노드에 대한 정보를 반환합니다.

#### Parameters:

None

#### Results:

결과 필드는 다음과 같은 배열이됩니다.

- `pubkey : <string>`-base-58 인코딩 문자열로 된 노드 공개 키-`gossip : <string>`-노드의
- `tpu : <string>`-TPU 노드의 네트워크 주소
- `tpu: <string>` - TPU network address for the node
- { "jsonrpc": "2.0", "id": 1, "method": "getClusterNodes"}
- | null`-소프트웨어 버전 노드의 rsion 또는 버전 정보를 사용할 수없는 경우`null`

#### Example:

Request:

```bash
curl http://loc alhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "gossip": "10.239.6.48:8001",
      "pubkey": "9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ",
      "rpc
      ": "10.239.6.48:8899",tpu": "10.239.6.48:8856
      "version": "" 1.0.0 c375ce1f "
    }
  ],
  "id ": 1
}
```

### getConfirmedBlock

원장에서 확인 된 블록에 대한 ID 및 트랜잭션 정보를 반환합니다

#### Parameters:

- `<u64>`-limit, as u64 integer
- `<u64>`-슬롯, u64 정수 -`<string>`-반환 된 각 트랜잭션에 대한 인코딩 ( "json", "jsonParsed", "base58"(_ slow _), "base64"). 매개 변수가 제공되지 않은 경우 기본 인코딩은 "json"입니다. 'jsonParsed'인코딩은 프로그램 별 명령어 파서를 사용하여 'transaction.message.instructions'목록에 더 많은 사람이 읽을 수 있고 명시적인 데이터를 반환하려고 시도합니다. "jsonParsed"가 요청되었지만 파서를 찾을 수없는 경우 지침은 일반 JSON 인코딩 (`accounts`,`data` 및`programIdIndex` 필드)으로 대체됩니다.

#### Results:

The result field will be an object with the following fields:

- -지정된 블록이 확인되지 않은 경우 -
- 결과 필드는 다음 필드가있는 객체가됩니다.
  - `-base-58로 인코딩 된 문자열로이 블록의 blockhash-`previousBlockhash :
  - `-base-58로 인코딩 된 문자열 로이 블록의 부모 블록 해시. 원장 정리로 인해 상위 블록을 사용할 수없는 경우이 필드는 "11111111111111111111111111111111"반환 -`parentSlot :
  - `parentSlot: <u64>` - the slot index of this block's parent
  - `transactions: <array>` - 다음을 담고있는 JSON 객체 어레이:
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) 객체, 인코딩 패리미터에 따라 JSON 및 인코디드 바이너리 데이터 포맷
    - `meta: <object>` - `null`이나 다음을 담고있는 트랜잭션 상태 메타데이터 객체:
      - `err: <object | null>` - 트랜잭션 실패 시 에러, 트랜잭션 성공 시 nulll. [TransactionError 정의](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - u64 integer 정수로 표현되는 트랜잭션 수수료
      - `preBalances: <array>` - 트랜잭션 처리 이전의 u64 계정 잔액 어레이
      - `postBalances: <array>` - 트랜잭션 처리 이후의 u64 계정 잔액 어레이
      - `innerInstructions: <array|undefined>` - [inner instructions](#inner-instructions-structure)의 리스트, 또는 트랜잭션 동안 내부 명령 리코딩이 활성화 되지 않은 경우 생략됨
      - `logMessages: <array>` - 스트링 로그 메시지 어레이, 또는 로그 메시지 리코딩이 트랜잭션동안 활성화되지 않은 경우 생략됨
      - DEPRECATED: `status: <object>` - 트랜잭션 상태
        - `"Ok": <null>` - 트랜잭션 성공
        - `"Err": <ERR>` - TransactionError로 트랜잭션 실패
  - `rewards: <array>` - 다음을 포함하는 JSON 객체 어레이:
    - `pubkey: <string>` - 보상을 받은 계정의 base-58로 인코딩된 공개키 스트링
    - `lamports: <i64>`- i64로 표현된 계정에 의해 들어오거나 나간 보상의 램포트 수
    - `postBalance: <u64>` - 보상 적용 후 램포트의 계정 잔액
    - `rewardType: <string|undefined>` - 보상 타입: "fee", "rent", "voting", "staking"
  - `blockTime: <i64 | null>` - 유닉스 타임스탬프로 표현된 예상 생성 시간 (유닉스 에폭 후 수 초). 사용할 수없는 경우 null

#### 예시:

요청:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}
'
```

결과:

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

#### 예제:

요청:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

결과:

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

#### 트랜잭션 구조

다른 블록체인과 비교하여 트랜잭션의 구조는 꽤나 다릅니다. 솔라나 상 트랜잭션에 대해서 [Anatomy of a Transaction](developing/programming-model/transactions.md)를 참고하세요.

트랜잭션의 JSON 구조는 다음과 같이 정의됩니다:

- `signatures: <array[string]>` - 트랜잭션에 적용된 base-58 인코딩 서명의 리스트. 리스트는 항상 `message.header.numRequiredSignatures`의 길이이며 빈칸으로 남지 않습니다. 인덱스 `i`의 서명은 `message.account_keys`의 `i` 인덱스의 공개키를 뜻합니다. 첫번째는 [transaction id](../../terminology.md#transaction-id)로서 사용됩니다.
- `message: <object>` - 트랜잭션 내용을 정의합니다.
  - `accountKeys: <array[string]>` - 명령과 서명을 포함한 base-58로 인코딩 된 공개키 리스트. `message.header.numRequiredSignatures` 공개키들이 반드시 먼저 트랜잭션에 서명해야 합니다.
  - `header: <object>` - 계정 타입과 트랜잭션이 요구하는 서명들에 대한 세부사항.
    - `numRequiredSignatures: <number>` - 유효한 트랜잭션을 위해 필요한 총 서명 수. 서명은 `message.account_keys`의 첫번째 `numRequiredSignatures`와 맞아야 합니다.
    - `numReadonlySignedAccounts: <number>` - `numReadonlySignedAccounts`의 마지막에 서명한 키들은 읽기전용 계정입니다. 프로그램은 단일한 역사증명 엔트리 내에 읽기전용 계정만을 담고있는 다수의 트랜잭션을 처리할 수 있지만, 램포트 잔액 변경 및 계정 데이터 변경에 대한 허가는 받지 못합니다. 동일한 읽기-쓰기 계정을 향한 트랜잭션은 순서대로 평가됩니다.
    - `numReadonlyUnsignedAccounts: <number>` - 서명되지 않은 키들의 마지막 `numReadonlyUnsignedAccounts`은 읽지전용 계정들입니다.
  - `recentBlockhash: <string>` - 트랜잭션 중복을 방지하고 시간을 부여하는 데 쓰이는 원장에 base-58 인코딩으로 해싱된 최근 블록.
  - `instructions: <array[object]>` - 성공 시 순차적으로 처리되고 하나의 아토믹 트랜잭션에 커밋될 프로그램 명령 리스트.
    - `programIdIndex: <number>` - Index into the `message.accountKeys` array indicating the program account that executes this instruction.
    - `accounts: <array[number]>` - 어떤 계정이 프로그램으로 패스되는지 보여주는 `message.accountKeys` 내 정렬된 인덱스 어레이.
    - `data: <string>` - base-58로 인코딩 된 프로그램 입력 데이터 스트링.

#### 내부 명령 구조

솔라나 런타임은 트랜잭션 처리 동안 인보크 되는 크로스 프로그램 명령을 기록하며 트랜잭션 명령 당 온체인에서 무엇이 실행되었는지를 더 높은 투명성과 함께 마련해 줍니다. 인보크 된 트랜잭션은 근원으로 하는 트랜잭션 명령에 따라 그룹화 되며 처리를 위해 정렬됩니다.

내부 명령의 JSON 구조는 다음 객체 리스트에 따라 정의됩니다:

- `index: number` - 내부 명령이 발원지로부터 나온 트랜잭션 명령의 인덱스
- `instructions: <array[object]>` - 단일 트랜잭션 명령동안 인보크 된 정렬된 내부 프로그램 명령 리스트.
  - `programIdIndex: <number>` - 해당 명령을 실행하는 프로그램 계정을 나타내는 `message.accountKeys` 어레이 내의 인덱스.
  - `accounts: <array[number]>` - 프로그램을 패스하는 계정을 나타내는 `message.accountKeys` 어레이 내 정렬된 인덱스.
  - `data: <string>` - base-58 스트링으로 인코딩된 프로그램 입력 데이터.

### getConfirmedBlocks

두 슬롯 간 확정된 블록의 리스트를 반환합니다

#### 패리미터:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - (optional) end_slot, as u64 integer

#### 결과:

결과 필드는 u64 정수의 리스트이며 이는 `message. accountKeys`와 `end_slot` 사이, 또는 마지막으로 확정된 블록입니다. 허용된 최대 벙위는 500,000 슬롯입니다.

#### 예시:

요청:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

결과:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### getConfirmedBlocksWithLimit

주어진 슬롯에서 시작되는 확정된 블록들의 리스트를 반환합니다.

#### 패리미터:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer

#### 결과:

결과 필드는 `start_slot`부터 `limit` 블록까지 포함되는 확정된 블록들의 어레이로서 u64 정수입니다.

#### 예시:

요청:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

결과:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getConfirmedSignaturesForAddress

**사용안함: getConfirmedSignaturesForAddress2를 사용하세요**

지정된 슬롯 범위 내에서 주소와 관련된 트랜잭션에 대해 확인 된 모든 서명 목록을 반환합니다. 허용되는 최대 범위는 10,000 개의 슬롯입니다.

#### 패리미터:

- `<string>` - base-58 인코딩 스트링으로 표현되는 계정 주소
- `<u64>` - 시작 슬롯.
- `<u64>` - 종료 슬롯.

#### 결과:

결과 필느는 아래의 어레이입니다:

- `<string>` - transaction signature as base-58 encoded string

은 확인 된 슬롯을 기준으로 가장 낮은 슬롯에서 가장 높은 슬롯까지 순서가 지정됩니다.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Conte nt-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

Result:

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
      "comm itment": "max"
    }
  ]
}
```

### getConfirmedSignaturesForAddress2

Returns confirmed signatures for transactions involving an address backwards in time from the provided signature or most recent confirmed block

#### Parameters:

- `<string>` - account address as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following fields:
  - `limit: <number>` - (optional) maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
  - `before: <string>` - (optional) start searching backwards from this transaction signature. If not provided the search starts from the top of the highest max confirmed block.
  - `until: <string>` - (optional) search until this transaction signature, if found before limit reached.

#### Results:

The result field will be an array of transaction signature information, ordered from newest to oldest transaction:

- `<object>`
  - `signature: <string>` - transaction signature as base-58 encoded string
  - `-이 블록의 상위 슬롯 인덱스-`transactions
  - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - Memo associated with the transaction, null if no memo is present

#### Example:

Request:

````bash
:```bash는
곱슬 곱슬에 http : // localhost를 : 8899 -X POST -H "콘텐츠 형식 : 응용 프로그램 / JSON"-d '
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
````

Result:

```json
{
  "jsonrpc": "2.0",
  "method": "logsNotification",
  "params": {
    "result": {
      "context ": {
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

### getConfirmedTransaction

Returns the current Transaction count from the led ger

#### Parameters:

- &lt;string&gt;`-58로 인코딩 된 문자열로 트랜잭션 서명 N 인코딩은 프로그램 별 명령어 파서를 사용하여` transaction.message.instructions`l ist. "jsonParsed"가 요청되었지만 파서를 찾을 수없는 경우 지침은 일반 JSON 인코딩 (`accounts`,`data`및`programIdIndex` 필드)으로 대체됩니다.
- `&lt;string&gt;`-반환 된 트랜잭션에 대한 인코딩 ( "json", "jsonParsed", "base58"(_ slow _) 또는 "base64"). 매개 변수가 제공되지 않은 경우 기본 인코딩은 JSON입니다.

#### Results:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | string | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### Example:

Request:

```bash
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJV YVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
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
        "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6Yjm ZLvpAByy2fj4nh6B"
      },
      "signatures": [
        "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
      ]
    }
  },
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
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "address": "FYjHNoFtSQ5uijKrZFyYAxvEr87hsKXkXcxkcmkBAf4r",
        " amount": "771",
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

### getEpochInfo

Returns information about the current epoch

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be an object with the following fields:

- `absoluteSlot: <u64>`, the current slot
- `blockHeight: <u64>`, the current block height
- `epoch: <u64>`, the current epoch
- `slotIndex: <u64>`, the current slot relative to the start of the current epoch
- `slotsInEpoch: <u64>`, the number of slots in this epoch

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

Result:

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

Returns epoch schedule information from this cluster's genesis config

#### Parameters:

None

#### Results:

The result field will be an object with the following fields:

- `slotsPerEpoch: <u64>`, the maximum number of slots in each epoch
- `leaderScheduleSlotOffset: <u64>`, the number of slots before beginning of an epoch to calculate a leader schedule for that epoch
- `warmup: <bool>`, whether epochs start short and grow
- `firstNormalEpoch: <u64>`, first normal-length epoch, log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH \* (2.pow(firstNormalEpoch) - 1)

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
'
```

Result:

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

Returns the fee calculator associated with the query blockhash, or `null` if the blockhash has expired

#### Parameters:

- `<string>` - query blockhash as a Base58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to:

- `<null>` - if the query blockhash has expired
- `<object>` - otherwise, a JSON object containing:
  - `feeCalculator: <object>`, `FeeCalculator` object describing the cluster fee rate at the queried blockhash

#### Example:

Request:

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

Result:

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

Returns the fee rate governor information from the root bank

#### Parameters:

None

#### Results:

The `result` field will be an `object` with the following fields:

- `burnPercent: <u8>`, Percentage of fees collected to be destroyed
- `maxLamportsPerSignature: <u64>`, Largest value `lamportsPerSignature` can attain for the next slot
- `minLamportsPerSignature: <u64>`, Smallest value `lamportsPerSignature` can attain for the next slot
- `targetLamportsPerSignature: <u64>`, Desired fee rate for the cluster
- `targetSignaturesPerSlot: <u64>`, Desired signature rate for the cluster

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFeeRateGovernor"}
'
```

Result:

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

Returns a recent block hash from the ledger, a fee schedule that can be used to compute the cost of submitting a transaction using it, and the last slot in which the blockhash will be valid.

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` set to a JSON object with the following fields:

- `blockhash: <string>` - a Hash as base-58 encoded string
- `feeCalculator: <object>` - FeeCalculator object, the fee schedule for this block hash
- `lastValidSlot: <u64>` - last slot in which a blockhash will be valid

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

Result:

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

Returns the slot of the lowest confirmed block that has not been purged from the ledger

#### Parameters:

None

#### Results:

- `<u64>` - Slot

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstAvailableBlock"}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": 250000, "id": 1 }
```

### getGenesisHash

Returns the genesis hash

#### Parameters:

None

#### Results:

- `<string>` - a Hash as base-58 encoded string

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC",
  "id": 1
}
```

### getHealth

Returns the current health of the node.

If one or more `--trusted-validator` arguments are provided to `solana-validator`, "ok" is returned when the node has within `HEALTH_CHECK_SLOT_DISTANCE` slots of the highest trusted validator, otherwise an error is returned. "ok" is always returned if no trusted validators are provided.

#### Parameters:

None

#### Results:

If the node is healthy: "ok" If the node is unhealthy, a JSON RPC error response is returned. The specifics of the error response are **UNSTABLE** and may change in the future

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

Healthy Result:

```json
{ "jsonrpc": "2.0", "result": "ok", "id": 1 }
```

Unhealthy Result (generic):

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

Unhealthy Result (if additional information is available)

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

Returns the identity pubkey for the current node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `identity`, the identity pubkey of the current node \(as a base-58 encoded string\)

#### Example:

Request:

```bash
{
  "jsonrpc": "2.0",
  "result": {
    "active": 124429280,
    "inactive": 73287840,
    "state": "activating"
  },
  " id": 1
}
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": { "identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN" },
  "id": 1
}
```

### getInflationGovernor

Returns the current inflation governor

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be a JSON object with the following fields:

- `initial: <f64>`, the initial inflation percentage from time 0
- `terminal: <f64>`, terminal inflation percentage
- `taper: <f64>`, rate per year at which inflation is lowered
- `foundation: <f64>`, percentage of total inflation allocated to the foundation
- `foundationTerm: <f64>`, duration of foundation pool inflation in years

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
'
```

Result:

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
          "base58 "
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

### getInflationRate

Returns the specific inflation values for the current epoch

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `total: <f64>`, total inflation
- `validator: <f64>`, inflation allocated to validators
- `foundation: <f64>`, inflation allocated to the foundation
- `epoch: <f64>`, epoch for which these values are valid

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

Result:

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

Returns the 20 largest accounts, by lamport balance

#### Parameters:

- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `filter: <string>` - filter results by account type; currently supported: `circulating|nonCirculating`

#### Results:

The result will be an RpcResponse JSON object with `value` equal to an array of:

- `<object>` - otherwise, a JSON object containing:
  - `address: <string>`, base-58 encoded address of the account
  - `lamports: <u64>`, number of lamports in the account, as a u64

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

Result:

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
        "addres s": "AXJADheGVp9cruP8WYu46oNkRbeASngN5fPCMVGQqNHa"
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

Returns the leader schedule for an epoch

#### Parameters:

- `<u64>` - (optional) Fetch the leader schedule for the epoch that corresponds to the provided slot. If unspecified, the leader schedule for the current epoch is fetched
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<null>` - if requested epoch is not found
- `<object>` - otherwise, the result field will be a dictionary of leader public keys \(as base-58 encoded strings\) and their corresponding leader slot indices as values (indices are relative to the first slot in the requested epoch)

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}
'
```

Result:

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

Returns minimum balance required to make account rent exempt.

#### Parameters:

- `<usize>` - account data length
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<u64>` - minimum lamports required in account

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

### getMultipleAccounts

Returns the account information for a list of Pubkeys

#### Parameters:

- `<array>` - An array of Pubkeys to query, as base-58 encoded strings
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd", or "jsonParsed". "base58" is limited to Account data of less than 128 bytes. "base64" will return base64 encoded data for Account data of any size. "base64+zstd" compresses the Account data using [Zstandard](https://facebook.github.io/zstd/) and base64-encodes the result. "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to "base64" encoding, detectable when the `data` field is type `<string>`.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### Results:

The result will be an RpcResponse JSON object with `value` equal to:

An array of:

- `<null>` - if the account at that Pubkey doesn't exist
- `<object>` - otherwise, a JSON object containing:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <[string, encoding]|object>`, data associated with the account, either as encoded binary data or JSON format `{<program>: <state>}`, depending on encoding parameter
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

Request:

```bash
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
          "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1Frje JcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
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

Result:

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

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNo tification",
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

### getProgramAccounts

Returns all accounts owned by the provided program Pubkey

#### Parameters:

- `<string>` - Pubkey of program, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd", or "jsonParsed". "base58"은 128 바이트 미만의 계정 데이터로 제한됩니다. "base64"는 모든 크기의 계정 데이터에 대해 base64로 인코딩 된 데이터를 반환합니다. "base64 + zstd"는 \[Zstandard\] (https://facebook.github.io/zstd/)를 사용하여 계정 데이터를 압축하고 결과를 base64로 인코딩합니다. "jsonParsed"인코딩은 프로그램 별 상태 파서를 사용하여 사람이 읽을 수 있고 명시적인 계정 상태 데이터를 반환하려고합니다. "jsonParsed"가 요청되었지만 파서를 찾을 수없는 경우 필드는 "base64"인코딩으로 대체되며,`data` 필드가`&lt;string&gt;`유형일 때 감지 할 수 있습니다.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.
  - (optional) `filters: <array>` - filter results using various [filter objects](jsonrpc-api.md#filters); account must meet all filter criteria to be included in results

##### Filters:

- `memcmp: <object>` - compares a provided series of bytes with program account data at a particular offset. Fields:

  - `offset: <usize>` - offset into program account data to start comparison
  - `bytes: <string>` - data to match, as base-58 encoded string

- `dataSize: <u64>` - compares the program account data length with the provided data size

#### Results:

The result field will be an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to `data: <[string,encoding]|object>`, data associated with the account, either as encoded binary data or JSON format `{<program>: <state>}`, depending on encoding parameter
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}
'
```

Result:

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

#### Example:

Request:

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

Result:

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

Returns a recent block hash from the ledger, and a fee schedule that can be used to compute the cost of submitting a transaction using it.

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

An RpcResponse containing a JSON object consisting of a string blockhash and FeeCalculator JSON object.

- `RpcResponse<object>` - RpcResponse JSON object with `value` field set to a JSON object including:
- `blockhash: <string>` - a Hash as base-58 encoded string
- `feeCalculator: <object>` - FeeCalculator object, the fee schedule for this block hash

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d 'i
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
'
```

Result:

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

Returns a list of recent performance samples, in reverse slot order. Performance samples are taken every 60 seconds and include the number of transactions and slots that occur in a given time window.

#### Parameters:

- `limit: <usize>` - (optional) number of samples to return (maximum 720)

#### Results:

An array of:

- `RpcPerfSample<object>`
  - `slot: <u64>` - Slot in which sample was taken at
  - `numTransactions: <u64>` - Number of transactions in sample
  - `numSlots: <u64>` - Number of slots in sample
  - `samplePeriodSecs: <u16>` - Number of seconds in a sample window

#### Example:

Request:

```bash
// Request
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getRecentPerformanceSamples", "params": [4]}
'
```

Result:

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

### getSnapshotSlo t

Returns the highest slot that the node has a snapshot for

#### Parameters:

None

#### Results:

- `<u64>` - Snapshot slot

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSnapshotSlot"}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": 100, "id": 1 }
```

Result when the node has no snapshot:

```json
{
  "jsonrpc": "2.0",
  "error": { "code": -32008, "message": "No snapshot" },
  "id": 1
}
```

### getSignatureStatuses

Returns the statuses of a list of signatures. Unless the `searchTransactionHistory` configuration parameter is included, this method only searches the recent status cache of signatures, which retains statuses for all active slots plus `MAX_RECENT_BLOCKHASHES` rooted slots.

#### Parameters:

- `<array>` - An array of transaction signatures to confirm, as base-58 encoded strings
- `<object>` - (optional) Configuration object containing the following field:
  - `searchTransactionHistory: <bool>` - if true, a Solana node will search its ledger cache for any signatures not found in the recent status cache

#### Results:

An RpcResponse containing a JSON object consisting of an array of TransactionStatus objects.

- `RpcResponse<object>` - RpcResponse JSON object with `value` field:

An array of:

- `<null>` - Unknown transaction
- `<object>`
  - `slot: <u64>` - The slot the transaction was processed
  - `confirmations: <usize | null>` - Number of blocks since signature confirmation, null if rooted, as well as finalized by a supermajority of the cluster
  - `err: <object | string | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus: <string | null>` - The transaction's cluster confirmation status; either `processed`, `confirmed`, or `finalized`. See [Commitment](jsonrpc-api.md#configuring-state-commitment) for more on optimistic confirmation.
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

Result:

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

#### Example:

Request:

```bash
curl http : // localhost : 8899 -X POST -H "Content-Type : application / json"-d '
  {
    "jsonrpc": "2.0",
    "id": (1)
    "방법": "getConfirmedSignaturesForAddress",
    "PARAMS"[
      "6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC",
      0,







100]}``결과``JSON
{
  "JSONRPC"을 "2.0",
  "결과"[
    "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby을
    ","4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr","4LQ14a7BYY27578Uj8LPCaVhSdJGLn9DJqnUJHpy95FMqdKf9acAhUhecPQNjNUy6VoNFUbvwYkPociFSf87cWbG

  ","ID


"1}``##

# getConfirmedSignaturesForAddress2

반환제공된 서명 또는 가장 최근의 확약 블록변수에서시간에 주소 이전 버전과 관련된 거래를위한 서명 확인

#### 매개 :
*`<문자열>`- 기본-58 인코딩 된 문자열로 계정 주소를
* '<개체>`- (선택 사항) 다음 필드를 포함하는 구성 객체 :
  *`limit : <number>`-(선택 사항) 반환 할 최대 트랜잭션 서명 (1 ~ 1,000, 기본값 : 1,000).
```

Result:

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

Returns the current slot the node is processing

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<u64>` - Current slot

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getSlotLeader

Returns the current slot leader

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<string>` - Node identity Pubkey as base-58 encoded string

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS",
  "id": 1
}
```

### getStakeActivation

Returns epoch activation information for a stake account

#### Parameters:

- `<string>` - Pubkey of stake account to query, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `epoch: <u64>` - epoch for which to calculate activation details. If parameter not provided, defaults to current epoch.

#### Results:

The result will be a JSON object with the following fields:

- `state: <string` - the stake account's activation state, one of: `active`, `inactive`, `activating`, `deactivating`
- `active: <u64>` - stake active during the epoch
- `inactive: <u64>` - stake inactive during the epoch

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": { "active": 197717120, "inactive": 0, "state": "active" },
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

Result:

```json
(optional) [Commitment](jsonrpc-api.md#configuri ng-state-commitment)
```

### getSupply

Returns information about the current supply.

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `total: <u64>` - Total supply in lamports
- `circulating: <u64>` - Circulating supply in lamports
- `nonCirculating: <u64>` - Non-circulating supply in lamports
- `nonCirculatingAccounts: <array>` - an array of account addresses of non-circulating accounts, as strings

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getSupply"}
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

Returns the token balance of an SPL Token account. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of Token account to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `uiAmount: <f64>` - the balance, using mint-prescribed decimals
- `amount: <string>` - the raw balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place

#### Example:

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
      "uiAmount": 98.64,
      "amount": "9864",
      "decimals": 2
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

Returns all SPL Token accounts by approved Del egate. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of account delegate to query, as base-58 encoded string
- `<object>` - Either:
  - `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  - `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### Results:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

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

Returns all SPL Token accounts by token owner. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of account owner to query, as base-58 encoded string
- `<object>` - Either:
  - `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  - `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### Results:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

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

Returns the 20 largest accounts of a particular SPL Token type. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects containing:

- `address: <string>` - the address of the token account
- `uiAmount: <f64>` - the token account balance, using mint-prescribed decimals
- `amount: <string>` - the raw token account balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place

#### Example:

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
    "foundation": 0.
```

### getTokenSupply

Returns the total supply of an SPL Token type. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `uiAmount: <f64>` - the total token supply, using mint-prescribed decimals
- `amount: <string>` - the raw total token supply without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
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
      "uiAmount": 1000,
      "amount": "100000",
      "decimals": 2
    }
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
{ "jsonrpc": "2.0", "result": { "solana-core": "1.6.0" }, "id": 1 }
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

- `votePubkey: <string>` - Vote account public key, as base-58 encoded string
- `nodePubkey: <string>` - Node public key, as base-58 encoded string
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

Result:

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

Returns the lowest slot that the node has information about in its ledger. This value may increase over time if the node is configured to purge older ledger data

#### Parameters:

None

#### Results:

- `u64` - Minimum ledger slot

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

Result:

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
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}
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

While the rpc service will reasonably retry to submit it, the transaction could be rejected if transaction's `recent_blockhash` expires before it lands.

Before submitting, the following preflight checks are performed:

1. The transaction signatures are verified
2. The transaction is simulated against the bank slot specified by the preflight commitment. On failure an error will be returned. Preflight checks may be disabled if desired. It is recommended to specify the same commitment and preflight commitment to avoid confusing behavior.

The returned signature is the first signature in the transaction, which is used to identify the transaction ([transaction id](../../terminology.md#transanction-id)). This identifier can be easily extracted from the transaction data before submission.

#### Parameters:

- `<string>` - fully-signed Transaction, as encoded string
- `<object>` - (optional) Configuration object containing the following field:
  - `skipPreflight: <bool>` - if true, skip the preflight transaction checks (default: false)
  - `preflightCommitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to use for preflight (default: `"max"`).
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
  - `commitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to simulate the transaction at (default: `"max"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

응답 출력은 다음과 같습니다. 다음 필드가있는 JSON 객체 :

- `-트랜잭션이 실패하면 오류, 트랜잭션이 성공하면 null. [TransactionError definitions] (https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)-`fee :
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

### setLogFilter

Sets the log filter on the validator

#### Parameters:

- `<string>` - the new log filter to use

#### Results:

- `<null>`

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"setLogFilter", "params":["solana_core=debug"]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": null, "id": 1 }
```

### validatorExit

If a validator boots with RPC exit enabled (`--enable-rpc-exit` parameter), this request causes the validator to exit.

#### Parameters:

None

#### Results:

- `<bool>` - Whether the validator exit operation was successful

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"validatorExit"}
'

```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

## Subscription Websocket

After connecting to the RPC PubSub websocket at `ws://<ADDRESS>/`:

- Submit subscription requests to the websocket using the methods below
- Multiple subscriptions may be active at once
- Many subscriptions take the optional [`commitment` parameter](jsonrpc-api.md#configuring-state-commitment), defining how finalized a change should be to trigger a notification. For subscriptions, if commitment is unspecified, the default value is `"singleGossip"`.

### accountSubscribe

Subscribe to an account to receive notifications when the lamports or data for a given account public key changes

#### Parameters:

- `<string>` - account Pubkey, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
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

- `<bool>` - unsubscribe success message

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

Subscribe to transaction logging. **UNSTABLE**

#### Parameters:

- `filter: <string>|<object>` - filter criteria for the logs to receive results by account type; currently supported:
  - "all" - subscribe to all transactions except for simple vote transactions
  - "allWithVotes" - subscribe to all transactions including simple vote transactions
  - `{ "mentions": [ <string> ] }` - subscribe to all transactions that mention the provided Pubkey (as base-58 encoded string)
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<integer>` - Subscription id \(needed to unsubscribe\)

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

- `<bool>` - unsubscribe success message

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

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programUnsubscribe", "params": [0] }
```

Result:

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
  "jsonrpc ":" 2.0
  ","결과
    "[{"ERR

      "NULL,"메모"널"서명
      ":"5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv
      ","슬롯

  "(114)},"ID


"1}``###

getConfirmedTransaction을

확인 된 트랜잭션에 대한 트랜잭션 세부 정보를 반환합니다.
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

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotUnsubscribe", "params": [0] }
```

Result:

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

Request:

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
