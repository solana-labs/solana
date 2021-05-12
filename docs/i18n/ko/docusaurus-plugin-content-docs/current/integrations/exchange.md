---
title: 거래소에 Solana 추가
---

이 가이드는 암호 화폐 거래소에 Solana의 기본 토큰 SOL을 추가하는 방법을 설명합니다.

## 노드 설정

고급 컴퓨터 / 클라우드 인스턴스에 최소 2 개의 노드를 설정하고, 최신 버전으로 즉시 업그레이드하고, 번들 모니터링 도구를 사용하여 서비스 운영을 주시하는 것이 좋습니다.

이 설정을 통해 다음을 수행 할 수 있습니다 .-데이터를 가져오고 인출 트랜잭션을 제출하기 위해 Solana 메인 넷-베타 클러스터에 대한 신뢰할 수있는 게이트웨이를 갖출 수 있습니다.- 보유 된 과거 블록 데이터의 양을 완전히 제어 할 수 있습니다 .-한 노드가실패하더라도 서비스 가용성을 유지합니다
- to have a trusted gateway to the Solana mainnet-beta cluster to get data and submit withdrawal transactions
- to have full control over how much historical block data is retained
- to maintain your service availability even if one node fails

Solana에노드는 빠른 블록과 높은 TPS를 처리하기 위해 상대적으로 높은 컴퓨팅 성능을 필요로합니다.  특정 요구 사항은 \[하드웨어 권장 사항\] (../ running-validator / validator-reqs.md)을 참조하십시오.

API를 노드를 실행하려면

1. [Install the Solana command-line tool suite](../cli/install-solana-cli-tools.md)
2. 시작 최소한 다음 매개 변수를 사용하여 검증

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

:`bash는
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction- 역사 \
  --limit은 원장 크기는 \
  --trusted - 검증 <VALIDATOR_ADDRESS> \
  --no-신뢰할
수없는-RPC`정의`--ledger`

`--entrypoint` 및`--expected-genesis-hash` 매개 변수는 모두 참여하는 클러스터에 따라 다릅니다. \[메인 넷 베타의 현재 매개 변수\] (../ clusters.md # example-solana-validator-command-line-2)

`--limit-ledger-size` 매개 변수를 사용하면 원장 \[파쇄\] (. ./terminology.md#shred) 노드가 디스크에 유지합니다. 이 매개 변수를 포함하지 않으면 유효성 검사기는 디스크 공간이 부족해질 때까지 전체 원장을 유지합니다.  기본값은 원장 디스크 사용량을 500GB 미만으로 유지하려고합니다.  원하는 경우`--limit-ledger-size`에 인수를 추가하여 디스크 사용량을 더 많이 또는 더 적게 요청할 수 있습니다. `--limit-ledger-size`에서 사용하는 기본 제한 값은`solana-validator --help`를 확인하세요.  맞춤 제한 값 선택에 대한 자세한 내용은 \[여기\] (https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26)를 참조하세요.

하나 이상의`--trusted-validator` 매개 변수를 지정하면 악성 스냅 샷에서 부팅하지 못하도록 보호 할 수 있습니다. \[신뢰할 수있는 밸리데이터로 부팅하는 값에 대해 자세히 알아보기\] (../ running-validator / validator-start.md # trusted-validators)

고려할 선택적 매개 변수 :

- 사용할 수있는 블록 확인하려면 [`getConfirmedBlocks` 요청]을 보내 이미 시작 슬롯 매개 변수로 처리 한 마지막 블록을 통과, (/ 클라이언트 / jsonrpc-api.md #의 getconfirmedblocks 개발)
- 원하는 원장 저장 위치, 그리고`--rpc - port` 노출하려는 포트에.

### 자동 다시 시작 및 모니터링자동으로 다시 시작

되도록 각 노드를 구성하는 것이 좋습니다. 가능한 한 적은 데이터를 놓치십시오. 솔라나 소프트웨어를 시스템 서비스로 실행하는 것은 훌륭한 옵션 중 하나입니다.

모니터링을 위해 밸리데이터을 모니터링하고`solana- '로 감지 할 수있는 [`solana-watchtower`] (https://github.com/solana-labs/solana/blob/master/watchtower/README.md)를 제공합니다. 밸리데이터 프로세스가 비정상입니다. Slack, Telegram, Discord 또는 Twillio를 통해 경고하도록 직접 구성 할 수 있습니다. 자세한 내용은`solana-watchtower --help`를 실행하세요.

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### 새 소프트웨어 릴리스 알림

새 소프트웨어를 자주 릴리스합니다 (약 1 주일에 릴리스). 때때로 최신 버전에는 호환되지 않는 프로토콜 변경 사항이 포함되어있어 처리 블록의 오류를 방지하기 위해 적시에 소프트웨어를 업데이트해야합니다.

모든 종류의 릴리스 (일반 및 보안)에 대한 공식 릴리스 발표는 [`# mb-announcement`] (https://discord.com/channels/428295358100013066/669406841830244375)라는 불화 채널을 통해 전달됩니다 (`mb`는 `mainnet-beta`).

스테이킹 밸리데이터과 마찬가지로 거래소 운영 밸리데이터은 정상적인 출시 발표 후 영업일 기준 1 ~ 2 일 이내에 가능한 한 빨리 업데이트 될 것으로 예상합니다. 보안 관련 릴리스의 경우 더 긴급한 조치가 필요할 수 있습니다.

### 원장 연속성

기본적으로 각 노드는 신뢰할 수있는 유효성 검사기 중 하나가 제공 한 스냅 샷에서 부팅됩니다. 이 스냅 샷은 체인의 현재 상태를 반영하지만 전체 내역 원장을 포함하지 않습니다. 노드 중 하나가 종료되고 새 스냅 샷에서 부팅되는 경우 해당 노드의 원장에 간격이있을 수 있습니다. 이 문제를 방지하려면`--no-snapshot-fetch` 매개 변수를`solana-validator` 명령에 추가하여 스냅 샷 대신 기록 원장 데이터를받습니다.

최초 부팅시`--no-snapshot-fetch` 매개 변수를 전달하지 마십시오. 제네시스 블록에서 노드를 완전히 부팅 할 수는 없습니다.  대신 스냅 샷에서 먼저 부팅 한 다음 재부팅을 위해`--no-snapshot-fetch` 매개 변수를 추가합니다.

나머지 네트워크의 노드에서 사용할 수있는 기록 원장의 양은 언제든지 제한된다는 점에 유의해야합니다.  일단 작동되면 밸리데이터이 상당한 다운 타임을 경험하면 네트워크를 따라 잡지 못할 수 있으며 신뢰할 수있는 밸리데이터으로부터 새 스냅 샷을 다운로드해야합니다.  이렇게하면 밸리데이터이 채울 수없는 과거 원장 데이터에 공백이 생깁니다.


### 유효성 검사기 포트 노출 최소화 유효성

검사기는 다른 모든 Solana 유효성 검사기의 인바운드 트래픽을 위해 다양한 UDP 및 TCP 포트를 열어야합니다.   이것이 가장 효율적인 작동 모드이며 강력하게 권장되지만 다른 Solana 유효성 검사기의 인바운드 트래픽 만 요구하도록 유효성 검사기를 제한 할 수 있습니다.

먼저`--restricted-repair-only-mode` 인수를 추가합니다.  이렇게하면 유효성 검사기가 나머지 유효성 검사기로부터 푸시를받지 않고 대신 블록에 대해 계속해서 다른 유효성 검사기를 폴링해야하는 제한된 모드에서 작동하게됩니다.  밸리데이터는 * Gossip * 및 * ServeR * ( "serve repair") 포트를 사용하는 다른 밸리데이터에게만 UDP 패킷을 전송하고 * Gossip * 및 * Repair * 포트에서 UDP 패킷 만 수신합니다.

Gossip * 포트는 양방향이며 유효성 검사기가 나머지 클러스터와 계속 연락 할 수 있도록합니다.  이제 Turbine이 비활성화되었으므로 유효성 검사기는 * ServeR *에서 네트워크의 나머지 부분에서 새 블록을 얻기위한 수리 요청을 전송합니다.  그러면 귀하의 밸리데이터은 다른 밸리데이터으로부터 * Repair * 포트에 대한 수리 응답을 받게됩니다.

유효성 검사기를 하나 이상의 유효성 검사기에서 요청하는 블록으로 만 제한하려면 먼저 해당 유효성 검사기에 대한 ID pubkey를 결정하고 각 PUBKEY에 대해`--gossip-pull-validator PUBKEY --repair-validator PUBKEY` 인수를 추가합니다.  이로 인해 유효성 검사기가 추가하는 각 유효성 검사기에서 리소스가 소모되므로 대상 유효성 검사기와상의 한 후에 만이 작업을 아껴서 수행하십시오.

이제 유효성 검사기는 명시 적으로 나열된 유효성 검사기와 * Gossip *, * Repair * 및 * ServeR * 포트에서만 통신해야합니다.

## 입금 계정 설정

Solana 계정은 온 체인 초기화가 필요하지 않습니다. 일단 SOL을 포함하면 존재합니다. 거래소에 입금 계정을 설정하려면 \[지갑 도구\] (../ wallet-guide / cli.md) 중 하나를 사용하여 Solana 키 쌍을 생성하기 만하면됩니다.

각 사용자에 대해 고유 한 예금 계좌를 사용하는 것이 좋습니다.

Solana 계정은 생성시 그리고 에포크 당 한 번 \[임대료\] (개발 / 프로그래밍 모델 /accounts.md#rent)가 청구되지만 SOL에 2 년 분량의 임대료가 포함 된 경우 임대료를 면제받을 수 있습니다. 예금 계좌의 최소 임대료 면제 잔액을 찾으려면 [`getMinimumBalanceForRentExemption` endpoint] (developing / clients / jsonrpc-api.md # getminimumbalanceforrentexemption) :

```bash
curl -X POST -H "Content- 유형 : application / json "-d '{"jsonrpc ":"2.0 ","id ": 1,"method ":"getMinimumBalanceForRentExemption ","params ": [0]}'localhost : 8899

{"jsonrpc ":" 2.0 ","result ": 890880,"id ": 1}
```

### 오프라인 계정오프라인

보안 강화를 위해 하나 이상의 컬렉션 계정에 대한 키를으로 유지할 수 있습니다. 그렇다면 \[오프라인 방법\] (../ offline-signing.md)을 사용하여 SOL을 핫 계정으로 이동해야합니다.

## 입금 듣기

When a user wants to deposit SOL into your exchange, instruct them to send a transfer to the appropriate deposit address.

### 블록에

사용자가 SOL을 거래소에 입금하려면 적절한 입금 주소로 송금하도록 지시하십시오.

- API를 노드에 [`getConfirmedSignaturesForAddress2` (개발 / 클라이언트 / jsonrpc-api.md # getconfirmedsignaturesforaddress2) 요청을 보내기

```bash
"NULL,"메모"널"서명
  ":"35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby
  ","슬롯
"(114)},
{"ERR

  "NULL,"메모"널"
  기호 ""4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr
  ","슬롯
"112},
{"ERR

  "NULL,"메모"널"서명
  ":"dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6
  ","슬롯
```

대한 폴링 거래소의 모든 예금 계좌를 추적하려면 Solana API 노드의 JSON-RPC 서비스를 사용하여 확인 된 각 블록에 대해 폴링하고 관심있는 주소를 검사합니다.

- For each block, request its contents with a [`getConfirmedBlock` request](developing/clients/jsonrpc-api.md#getconfirmedblock):

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

`preBalances` 및`postBalances` 필드를 사용하면 전체 트랜잭션을 구문 분석하지 않고도 모든 계정의 잔액 변경을 추적 할 수 있습니다. 'accountKeys'목록에 색인 된 \[lamports\] (../ terminology.md # lamport)에있는 각 계정의 시작 및 종료 잔액을 나열합니다. 예를 들어이자가 '47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi'인 경우 입금 주소가 218099990000-207099990000 = 11000000000 lamports = 11 SOL 인

만일 트랜잭션 타입 및 기타 세부사항에 대한 추가정보가 필요하다면, RPC에서 바이너리 포맷의 블록을 요청하고 [Rust SDK](https://github.com/solana-labs/solana) 나 [Javascript SDK](https://github.com/solana-labs/solana-web3.js)로 parsing할 수 있습니다.

### 주소 내역

특정 주소의 거래 내역을 조회 할 수도 있습니다. 이것은 일반적으로 모든 슬롯에서 모든 입금 주소를 추적하는 실행 가능한 방법은 아니지만, 특정 기간 동안 몇 개의 계정을 검사하는 데 유용 할 수 있습니다.

- Send a [`getConfirmedSignaturesForAddress2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) request to the api node:

```bash
:```bash는
컬 -X POST -H "콘텐츠 형식 : 응용 프로그램 / JSON"-d '{ "jsonrpc": "2.0", "id": 1, "method": "getConfirmedSignaturesForAddress2", "params": [ "6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", { "limit": "3}]} 'localhost : 88993}]}'localhost : 8899
```

- For each signature returned, get the transaction details by sending a [`getConfirmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction) request:

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

## 인출 전송

사용자의 SOL 인출 요청을 수용하려면 Solana 전송 트랜잭션을 생성해야합니다. , 클러스터로 전달할 api 노드로 보냅니다.

### 동기

Sending a synchronous transfer to the Solana cluster allows you to easily ensure that a transfer is successful and finalized by the cluster.

Solana의 명령 줄 도구는 전송 트랜잭션을 생성, 제출 및 확인하는 간단한 명령 'solana transfer'를 제공합니다. 기본적으로이 메서드는 트랜잭션이 클러스터에 의해 완료 될 때까지 stderr에서 진행률을 대기하고 추적합니다. 트랜잭션이 실패하면 트랜잭션 오류를보고합니다.

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --keypair <KEYPAIR> --url http://localhost:8899
```

The [Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) offers a similar approach for the JS ecosystem. Use the `SystemProgram` to build a transfer transaction, and submit it using the `sendAndConfirmTransaction` method.

### 인출을위한 사용자 제공 계정 주소 확인

For greater flexibility, you can submit withdrawal transfers asynchronously. In these cases, it is your responsibility to verify that the transaction succeeded and was finalized by the cluster.

**Note:** Each transaction contains a [recent blockhash](developing/programming-model/transactions.md#blockhash-format) to indicate its liveness. It is **critical** to wait until this blockhash expires before retrying a withdrawal transfer that does not appear to have been confirmed or finalized by the cluster. Otherwise, you risk a double spend. See more on [blockhash expiration](#blockhash-expiration) below.

-반환 된 각 서명에 대해 [`getConfirmedTransaction`] (developing / clients / jsonrpc-api.md # getconfirme dtransaction) 요청 :

```bash
spl-token create-account <TOKEN_MINT_ADDRESS>소유
```

In the command-line tool, pass the `--no-wait` argument to send a transfer asynchronously, and include your recent blockhash with the `--blockhash` argument:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

Solana 클러스터에 동기 전송을 전송하면 전송이 성공적이고 클러스터에 의해 완료되었는지 쉽게 확인할 수 있습니다.

#### Blockhash 만료

Get the status of a batch of transactions using the [`getSignatureStatuses` JSON-RPC endpoint](developing/clients/jsonrpc-api.md#getsignaturestatuses). The `confirmations` field reports how many [confirmed blocks](../terminology.md#confirmed-block) have elapsed since the transaction was processed. If `confirmations: null`, it is [finalized](../terminology.md#finality).

```bash
:```bash는
컬 -X POST -H "Content-Type : application / json"-d '{ "jsonrpc": "2.0", "id": 1, "method": "getConfirmedBlocks", "params": [5]}'localhost : 8899
```

#### Blockhash Expiration

[`getFees` 엔드 포인트를 사용하여 출금 트랜잭션에 대한 최근 블록 해시를 요청할 때] ( development / clients / jsonrpc-api.md # getfees) 또는 'solana fee'의 경우 응답에는 blockhash가 유효한 마지막 슬롯 인 'lastValidSlot'이 포함됩니다. [`getSlot` 쿼리] (developing / clients / jsonrpc-api.md # getslot);로 클러스터 슬롯을 확인할 수 있습니다. 클러스터 슬롯이`lastValidSlot`보다 크면 해당 블록 해시를 사용하는 인출 트랜잭션이 성공해서는 안됩니다.

또한 blockhash를 매개 변수로 사용하여 [`getFeeCalculatorForBlockhash`] (developing / clients / jsonrpc-api.md # getfeecalculatorforblockhash) 요청을 전송하여 특정 blockhash가 여전히 유효한지 다시 확인할 수 있습니다. 응답 값이 null이면 블록 해시가 만료되고 인출 트랜잭션이 성공해서는 안됩니다.

### Validating User-supplied Account Addresses for Withdrawals

인출은 되돌릴 수 없으므로 사용자 자금의 우발적 인 손실을 방지하기 위해 인출을 승인하기 전에 사용자가 제공 한 계정 주소를 확인하는 것이 좋습니다.

Solana의 일반 계정 주소는 256 비트 ed25519 공개 키의 Base58 인코딩 문자열입니다. 모든 비트 패턴이 ed25519 곡선에 대해 유효한 공개 키가 아니므로 사용자가 제공 한 계정 주소가 최소한 올바른 ed25519 공개 키인지 확인할 수 있습니다.

#### Java

다음은 사용자 제공 주소를 유효한 ed25519 공개 키로 확인하는 Java 예제입니다

다음 코드 샘플에서는 Maven을 사용하고 있다고 가정합니다.

`pom.xml` :

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
      <groupId> io.github.novacrypto </ groupId>
      <artifactId> Base58 </ artifactId>
      <version> 0.1.3 </ version>
  </ dependency>
  <dependency>
      <groupId> cafe.cryptography </ groupId>
      <artifactId> curve25519-elisabeth </ artifactId>
      <version> 0.1.0 </ version>
  </ dependency>
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

## SPL 토큰 표준 지원

\[SPL Token\] (https://spl.solana.com/token)은 Solana 블록체인에서 래핑 / 합성 토큰 생성 및 교환을위한 표준입니다.

SPL 토큰 워크 플로는 네이티브 SOL 토큰의 워크 플로와 비슷하지만이 섹션에서 설명 할 몇 가지 차이점이 있습니다.

### Token Mints

SPL 토큰의 각 * 유형 *은 * mint * 계정을 생성하여 선언됩니다.  이 계정은 공급, 소수 자릿수 및 민트를 제어하는 ​​다양한 권한과 같은 토큰 기능을 설명하는 메타 데이터를 저장합니다.  각 SPL 토큰 계정은 관련 민트를 참조하며 해당 유형의 SPL 토큰과 만 상호 작용할 수 있습니다.

### # 예제

SPL 토큰 계정은`spl-token` 명령 줄 유틸리티를 사용하여 쿼리하고 수정합니다. 이 섹션에 제공된 예제는 로컬 시스템에 설치되었는지에 따라 다릅니다.

`spl-token`은 Rust`cargo` 명령 줄 유틸리티를 통해 Rust \[crates.io\] (https://crates.io/crates/spl-token)에서 배포됩니다. 최신 버전의`cargo`는 \[rustup.rs\] (https://rustup.rs)에서 귀하의 플랫폼을위한 편리한 원 라이너를 사용하여 설치할 수 있습니다. cargo`가 설치되어`하면`SPL-token`는 다음 명령을 사용하여 얻을 수 있습니다

```
cargo install spl-token-cli
```

설치된 버전을 확인하세요

```
spl-token --version
```

그러면 다음과 같이 뜨게 됩니다.

```text
spl-token-cli 2.0.1
```

### 기타 고려 사항

있습니다```SPL 토큰 -

1. SPL 토큰은 필수 계정 일정량의 토큰이 입금되기 전에 생성됩니다.   토큰 계정은`spl-token create-account` 명령을 사용하여 명시 적으로 만들거나`spl-token transfer --fund-recipient ...`명령을 사용하여 암시 적으로 만들 수 있습니다.
1. SPL 토큰 계정은 존재하는 동안 \[rent-exempt\] (developing / programming-model / accounts.md # rent-exmption) 상태를 유지해야하므로 계정 생성시 소량의 기본 SOL 토큰을 예치해야합니다. SPL 토큰 v2 계정의 경우이 금액은 0.00203928 SOL (2,039,280 램프 포트)입니다.

#### Command Line
같은 발생한다
1. 주어진 민트와 연결
1. 펀딩 계정의 키 쌍

```
spl-token create-account <TOKEN_MINT_ADDRESS>
```

#### Example
```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

또는
```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### Checking an Account's Balance

#### Command Line
```
spl-token balance <TOKEN_ACCOUNT_ADDRESS>
```

#### Example
```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### Token Transfers

버전은``````텍스트 SPL-토큰 CLI는 2.0.1```###

The recipient address however can be a normal wallet account.  If an associated token account for the given mint does not yet exist for that wallet, the transfer will create it provided that the `--fund-recipient` argument as provided.

#### Command Line
```
$ spl-token transfer --fund-recipient <교환 토큰 계정> <인출 금액> <인출 주소>
```

#### Example
```
#### Example
```

### 예치
각 `(user, mint)` 짝은 온체인에 구별되는 계정을 필요로 하며, 거래소들은 토큰 계정 배치를 미리 생성하고 유저 요청 시 할당할 것을 권장합니다. 모든 해당 계정은 거래소가 컨트롤하는 키페어 소유여야 합니다.

입금 거래 모니터링은 위에서 설명한 \[block polling\] (# poll-for-blocks) 방법을 따라야합니다. SPL 토큰 \[Transfer\] (https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92)을 발행하는 성공적인 거래를 위해 각 새 블록을 스캔해야합니다. 또는 \[Transfer2\] (https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) 명령으로 사용자 계정을 참조한 다음 \[토큰 계정을 쿼리합니다. balance\] (developing / clients / jsonrpc-api.md # gettokenaccountbalance) 업데이트.

예`$
SPL 토큰 전송 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
전송한 토큰
  보낸사람 :
  6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN받는사람 : 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
서명:
3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj`

### 출금
입금 각각`(사용자, 민트)가`쌍 체인에 별도의 계정을 필요로하기 때문에, 교환이 사전에 토큰 계정의 배치를 작성하고 사용자에게 할당하는 것이 좋습니다 요청시.

행`SPL
토큰 균형
<TOKEN_ACCOUNT_ADDRESS>`확인

출금 주소로부터 관련된 올바른 민팅 토큰 계정이 결정되고 해당 계정으로 전송이 발생합니다.  Note that it's possible that the associated token account does not yet exist, at which point the exchange should fund the account on behalf of the user.  For SPL Token v2 accounts, funding the withdrawal account will require 0.00203928 SOL (2,039,280 lamports).

Template `spl-token transfer` command for a withdrawal:
```
$ spl-token transfer --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### Other Considerations

#### Freeze Authority
규정 준수를 위해 SPL 토큰 발행 기관은 민트와 관련하여 생성 된 모든 계정에 대해 "권한 동결"을 선택적으로 보유하도록 선택할 수 있습니다.  이렇게하면 주어진 계정의 자산을 마음대로 \[고정\] (https://spl.solana.com/token#freezing-accounts)하여 해동 될 때까지 계정을 사용할 수 없게됩니다. 이 기능이 사용 중이면 동결 기관의 pubkey가 SPL 토큰의 민트 계정에 등록됩니다.

## 통합

테스트 메인 넷 베타에서 프로덕션으로 이동하기 전에 Solana devnet 및 testnet \[clusters\] (../ clusters.md)에서 전체 워크 플로를 테스트해야합니다. Devnet은 가장 개방적이고 유연하며 초기 개발에 이상적이며 testnet은보다 현실적인 클러스터 구성을 제공합니다. devnet과 testnet은 모두 수도꼭지를 지원하고`solana airdrop 10`을 실행하여 개발 및 테스트를위한 devnet 또는 testnet SOL을 얻습니다.
