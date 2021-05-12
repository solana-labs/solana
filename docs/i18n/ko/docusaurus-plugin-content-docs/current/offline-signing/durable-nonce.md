---
title: Durable Transaction Nonces
---

는 트랜잭션 [`recent_blockhash`] (developing / programming-model / transactions.md # recent-blockhash)의 일반적인 짧은 수명을 처리하는 메커니즘입니다. 그것들은 솔라나 프로그램으로 구현되며, 그 메커니즘은 \[제안\] (../ implemented-proposals / durable-tx-nonces.md)에서 읽을 수 있습니다.

## 사용 예

내구성있는 nonce CLI 명령에 대한 전체 사용 세부 정보는 \[CLI 참조\] (../ cli / usage.md)에서 찾을 수 있습니다.

### Nonce 계정 생성

임시 계정에 대한 임시권한은 선택적으로 다른 계정에 할당 될 수 있습니다. 이렇게하면 새 권한은 계정 생성자를 포함하여 이전 권한에서 nonce 계정에 대한 모든 권한을 상속합니다. 이 기능을 사용하면 더 복잡한 계정 소유권 컨트랙트 및 키 쌍과 연결되지 않은 파생 계정 주소를 만들 수 있습니다. 은`--nonce-권한 <AUTHORITY_KEYPAIR은> '인수는이 계정을 지정하는 데 사용되는 다음과 같은 명령에 의해 지원됩니다

- `할당 권한 다시 할당 와 생성 후 거버넌스 계정`
- `new-nonce`
- `인출 와 비표 계정에서이야`
- `authorize-nonce-account`

### 저장된 Nonce 값 쿼리

영구 트랜잭션 nonce 기능은 계정을 사용하여 다음 nonce 값을 저장합니다. 영구 임시 계정은 \[rent-exempt\] (../ implemented-proposals / rent.md # two-tiered-rent-regime)이어야하므로이를 달성하려면 최소 잔액을 보유해야합니다.

`만들-거버넌스 - account`를 -`새로운 nonce`을 -'철수-부터 거버넌스 - account`

- Command

```bash
밥<code>bash는
$ 솔라-keygen은 새로운 -o alice.json의
$ 솔라-keygen은 새로운 - 오 nonce.json
$ 솔라-keygen은 새로운 -o
bob.json</code>
```

- Output

```text
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ```###
```

> \[전체 사용 문서\] (../ cli / usage.md # solana-create-nonce-account)

> [Full usage documentation](../cli/usage.md#solana-create-nonce-account)

### 저장된 논스 값

내구성있는 nonce 트랜잭션을 생성하려면 저장된 nonce를 전달해야합니다. 서명 및 제출시`--blockhash` 인수에 대한 값으로 값을 지정합니다. 에 현재 저장 거버넌스 값을 구합니다

- Command

```bash
solana nonce nonce-keypair.json
```

- Output

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU```&#062;
```

> [Full usage documentation](../cli/usage.md#solana-get-nonce)

### 디스플레이 난스

nonce 값 계정이 먼저 발생 새로운 키 쌍에 의해 생성되는 다음 체인에 계정을 생성

- Command

```bash
solana new-nonce nonce-keypair.json
```

- Output

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK```&#062;
```

> [Full usage documentation](../cli/usage.md#solana-new-nonce)

### Nonce 계정에서 자금 인출 자금

nonce 값 계정이 먼저 발생 새로운 키 쌍에 의해 생성되는 다음 체인에 계정을 생성

- Command

```bash
<code>bash는
솔라 거버넌스 - 계정 거버넌스 -
keypair.json</code>
```

- Output

```text
출력<code>텍스트
밸런스 : 0.5 SOL
최소균형이 필요합니다 : 0.00136416 SOL의
비표 :
DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS을</code>&#062;
```

> [Full usage documentation](../cli/usage.md#solana-nonce-account)

### 에 새 권한

명령`배시
솔라-keygen은 새로운 -o 논스-keypair.json을
만들-거버넌스 - 계정 솔라 난스-keypair.json
1`

- Command

```bash
명령<code>bash는
솔라 철회-부터 거버넌스-계정 거버넌스 - keypair.json ~ /의 .config / 솔라 / id.json
0.5</code>
```

- Output

```text
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6eHUAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV을```####
```

> 전체 잔액을 인출하여 임시 계정 폐쇄

> [Full usage documentation](../cli/usage.md#solana-withdraw-from-nonce-account)

### Durable Nonce를 사용한 지불 예제Durable nonce를 사용하여

2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL```>

- Command

```bash
여기에서는 별도의 [nonce Authority] (# nonce-authority)가 사용되지 않으므로<code>alice.json</code>은 임시 계정대한 완전한 권한을
```

- Output

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT```&#062;
```

> [Full usage documentation](../cli/usage.md#solana-authorize-nonce-account)

## Durable Nonce지원하는 기타 명령Durable Nonce

>

- `--nonce`, nonce 값을 저장하는 계정 지정-`--nonce
-authority`, 선택적 지정 \[nonce Authority\] (# nonce-authority)
- `--nonce-authority`, specifies an optional [nonce authority](#nonce-authority)

출력```텍스트

- [`pay`](../cli/usage.md#solana-pay)
- [`delegate-stake`](../cli/usage.md#solana-delegate-stake)
- [`deactivate-stake`](../cli/usage.md#solana-deactivate-stake)

### Example Pay Using Durable Nonce

여기에서는 Alice가Bob 1 SOL을 지불하는 것을 보여줍니다. 절차는내구성 난스를 지원하는 모든 하위 명령에 대해 동일합니다

#### - 만들기 계정,

2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL```>

```bash
명령<code>bash는
솔라 거버넌스 거버넌스 -
keypair.json</code>
```

#### -기금 앨리스의 계정

앨리스는 논스 계정 생성 및 밥에게 보낼 자금이 필요할 것입니다. 앨리스에게 SOL을 에어드랍 하세요.

```bash
$ solana airdrop -k alice.json 10
10 SOL
```

#### 우리가앨리스에 대한 몇 가지 계정이 필요 먼저  앨리스의 비표와

이제 앨리스를 비표 계정이 필요합니다. 생성

> 시도>,기억`alice.json`는 [거버넌스 기관이 예에서 (# 거버넌스입니다.

```bash
F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7``````bash는
$의 솔라 지불 -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 1
```

#### - A failed first attempt to pay Bob

밥을 지불앨리스 시도를실패했지만서명하는 데 시간이 너무 오래 걸립니다. 지정된 blockhash가 만료되고 트랜잭션이 실패합니다

```bash
<code>bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 1
[2020-01-02T18 : 48 : 28.462911000Z ERROR solana_cli :: cli] Io (Custom {kind : 기타 오류 : "거래 \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV \ "실패 : 없음"})
오류 : 이오 (사용자 정의 {종류 : 기타, 오류 : "거래 \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV \ "실패 :
없음을"})</code>
```

#### -Nonce를 구출하세요!

명령`bash는
솔라 새로운 넌스 넌스 -
keypair.json`

> Remember, `alice.json` is the [nonce authority](#nonce-authority) in this example

```bash
SOL``````bash는
$ 솔라 거버넌스 - 계정 nonce.json
1 SOL :균형
최소밸런스필수 : 0.00136416 SOL의
거버넌스 :
6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN```
```

```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 1
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ
```

#### -성공!

The transaction succeeds! 거래가 성공했습니다!밥은 새 앨리스와 앨리스의 저장 거버넌스의 발전에서 1 SOL을 수신

```bash
값```bash는
$의 솔라 균형 -k bob.json
1
```

```bash
거버넌스-권한)```bash는
$ 솔라 거버넌스 - 계정  JSON
밸런스: 1 개 SOL
최소균형이 필요합니다 : 0.00136416 SOL의
거버넌스 :
```
