---
title: 오프라인 트랜잭션 서명
---

일부 보안 모델은 서명 키를 유지해야하며 따라서 서명 프로세스가 트랜잭션 생성 및 네트워크 브로드 캐스트와 분리되어 있어야합니다. 예는 다음과 같습니다

- Collecting signatures from geographically disparate signers in a [multi-signature scheme](cli/usage.md#multiple-witnesses)
- Signing transactions using an [airgapped](<https://en.wikipedia.org/wiki/Air_gap_(networking)>) signing device

.-\[다중 서명 체계\] (cli / usage.md # multiple-witnesses)에서 지리적으로 다른 서명자로부터 서명 수집 -[airgapped]를 사용하여 트랜잭션 서명 (https://en.wikipedia.org/wiki/ Air*gap* (networking)) 서명 장치이

## 오프라인 서명을 지원하는 명령

문서는 Solana의 CLI를 사용하여 트랜잭션을 별도로 서명하고 제출하는 방법에 대해 설명합니다.

- [`create-stake-account`](cli/usage.md#solana-create-stake-account)
- [`deactivate-stake`](cli/usage.md#solana-deactivate-stake)
- [`명령`](cli/usage.md#solana-delegate-stake)
- [`split-stake`](cli/usage.md#solana-split-stake)
- [`stake-authorize`](cli/usage.md#solana-stake-authorize)
- [`stake-set-lockup`](cli/usage.md#solana-stake-set-lockup)
- [`transfer`](cli/usage.md#solana-transfer)
- [`withdraw-stake`](cli/usage.md#solana-withdraw-stake)

## 오프라인 거래 서명 오프라인 거래에 서명

현재 다음 명령은 오프라인 서명을 지원합니다

1. 1.`--sign-only`는 클라이언트가 서명 된 트랜잭션을 네트워크에 제출하지 못하도록합니다. 대신 pubkey / signature 쌍이 stdout에 인쇄됩니다.
2. 2.`--blockhash BASE58_HASH`, 호출자가 트랜잭션의`recent_blockhash` 필드를 채우는 데 사용되는 값을 지정할 수 있습니다. 이는 다음과 같은 여러 목적에 사용됩니다. _ 네트워크에 연결하고 RPC를 통해 최근 블록 해시를 쿼리 할 필요가 없습니다. _ 서명자가 다중 서명 체계에서 블록 해시를 조정할 수 있도록합니다.

### 예 : 결제오프라인 서명

.-[`create-stake-account`] (cli / usage.md # solana-create-stake-account) -[`deactivate-stake`] ( cli / usage.md # solana-deactivate-stake) -[`delegate-stake`] (cli / usage.md # solana-delegate-stake) -[`split-stake`] (cli / usage.md # solana- split-stake) -[`stake-authorize`] (cli / usage.md # solana-stake-authorize) -[`stake-set-lockup`] (cli / usage.md # solana-stake-set-lockup) -[`transfer`] (cli / usage.md # solana-transfer) -[`withdraw-stake`] (cli / usage.md # solana-withdraw-stake)

````bash
```text
solana @ offline1 $ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign 전용 \
    --keypair fee_payer.json \
    --from
674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL``출력
````

하려면 명령 줄에서 다음 인수

```text

'' '텍스트
Blockhash : 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
서명자 (Pubkey = 서명)
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy = ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
결석 서명자 (Pubkey) :
```

## Submitting Offline Signed Transactions to the Network

To submit a transaction that has been signed offline to the network, pass the following arguments on the command line

1. `--blockhash BASE58_HASH`, must be the same blockhash as was used to sign
2. ommand1.`--blockhash BASE58_HASH`는에 서명하는 데 사용 된 것과 동일한 blockhash 여야합니다 2.`--signer BASE58_PUBKEY = BASE58_SIGNATURE`, 오프라인 서명자마다 하나씩. 여기에는 로컬 키 쌍으로 서명하는 대신 트랜잭션에 직접 pubkey / signature 쌍이 포함됩니다.

### 예 : 오프라인 서명 결제제출

명령에

````bash
```bash
solana @ online $ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    - 서명자 FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j =
    4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN받는사람 keypair.json
````

` ``bash는 솔라 @ 오프라인 $ 솔라 지불 --sign 전용 --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \받는사람 keypair.json

````text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN```##
````

## 다중 세션이상 오프라인

오프라인 서명서명은여러 세션에 걸쳐 일어날 수 있습니다. 이 시나리오에서는 각 역할에 대해없는 서명자의 공개 키를 전달합니다. 지정되었지만 서명이 생성되지 않은 모든 pubkey는 오프라인 서명 출력에없는 것으로 나열됩니다

### . ### 예 : 두 개의 오프라인 서명 세션을 사용한 전송

Blockhash : 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF 서명자 (Pubkey = 서명) : FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j = 4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

```text
제출)<code>텍스트
솔라 @ 온라인 $ 솔라 전송 Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL = 3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee 지불 자 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy =
ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi</code>출력
```

출력

````text
2)```텍스트
Blockhash : 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
서명자 (Pubkey = 서명) :
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL = 3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ
결석 서명자 (Pubkey) :
````

{ "blockhash" "5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF", "서명자": [ "FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j = 4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}

```text
2)<code>텍스트
솔라 @ offline2 $ Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut (10)를 전송 솔라 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign 전용 \
    --keypair from.json \
    --에프 EE-지불
3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy</code>출력
```

'```##를

````text
674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL```명령
````

출력

```text
solana@online$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

Output (Online Submission)

````text
3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy```명령
````

## ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi```##가입하는 데 더 많은 시간을 구매

솔라나 거래는`recent_blockhash` 필드에 blockhash에서 슬롯의 숫자 내에서 서명 및 네트워크에서 허용되어야합니다 일반적으로(~이 글을 쓰는 시점에서 2 분). 서명 절차가 이보다 오래 걸리면 \[Durable Transaction Nonce\] (offline-signing / durable-nonce.md)가 필요한 추가 시간을 제공 할 수 있습니다.
