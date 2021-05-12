---
title: Durable Transaction Nonces
---

## 문제

재생을 방지하기 위해 Solana 트랜잭션은 "최근"블록 해시 값으로 채워진 nonce 필드를 포함합니다. 너무 오래된 블록 해시가 포함 된 트랜잭션 (이 글을 쓰는 시점에서 ~ 2 분)은 네트워크에서 유효하지 않은 것으로 거부됩니다. 안타깝게도 관리 서비스와 같은 특정 사용 사례는 트랜잭션에 대한 서명을 생성하는 데 더 많은 시간이 필요합니다. 이러한 잠재적 인 오프라인 네트워크 참여자를 활성화하려면 메커니즘이 필요합니다.

## 요구 사항

1. 트랜잭션의 서명은 nonce 값을 포함 해야합니다.
2. nonce는 키 공개에 서명하는 경우에도 재사용 할 수 없어야합니다.

## 컨트랙트 기반 솔루션

여기서는 문제에 대한 컨트랙트 기반 솔루션을 설명합니다. 클라이언트는 트랜잭션의`recent_blockhash` 필드에서 나중에 사용할 수 있도록 nonce 값을 "숨길"수 있습니다. 이 접근 방식은 일부 CPU ISA에 의해 구현 된 Compare and Swap 원자 명령과 유사합니다.

영구 임시 값을 사용할 때 클라이언트는 먼저 계정 데이터에서 해당 값을 쿼리해야합니다. 트랜잭션은 이제 정상적인 방식으로 구성되지만 다음과 같은 추가 요구 사항이 있습니다.

1. 'recent_blockhash'필드에서 내구성있는 nonce 값이 사용됩니다.
2. 'AdvanceNonceAccount'명령이 트랜잭션에서 처음 발행됩니다.

### 컨트랙트 메커니즘

TODO : svgbob 이것을 플로우 차트에 넣습니다

```text
```text
Start
Create Account
  state = Uninitialized
NonceInstructionUninitialized
  if state ==if state == Uninitialized
    if account.balance
```

프로그램 아래 거버넌스 계정을 만들어이 기능 시작을 사용하고자하는 클라이언트. 이 계정은 저장된 해시가없는 'Uninitialized'상태이므로 사용할 수 없습니다.

새로 생성 된 계정을 초기화하려면`InitializeNonceAccount` 명령을 발행해야합니다. 이 명령어는 계정의 \[authority\] (../ offline-signing / durable-nonce.md # nonce-authority)의 'Pubkey'매개 변수 하나를 사용합니다. Nonce 계정은 기능의 데이터 지속성 요구 사항을 충족하기 위해 \[rent-exempt\] (rent.md # two-tiered-rent-regime)이어야하며, 따라서 초기화하기 전에 충분한 램프 포트를 예치해야합니다. 성공적으로 초기화되면 클러스터의 가장 최근 블록 해시가 지정된 임시 권한 'Pubkey'와 함께 저장됩니다.

`AdvanceNonceAccount` 명령은 계정의 저장된 nonce 값을 관리하는 데 사용됩니다. 클러스터의 가장 최근 블록 해시를 계정의 상태 데이터에 저장하고 이미 저장된 값과 일치하면 실패합니다. 이 검사는 동일한 블록 내에서 트랜잭션을 재생하는 것을 방지합니다.

임시 계정의 \[rent-exempt\] (rent.md # two-tiered-rent-regime) 요구 사항으로 인해 사용자 지정 인출 지침이 계정에서 자금을 이동하는 데 사용됩니다. `WithdrawNonceAccount` 명령은 단일 인수를 취하고, 인출을 위해 램프 포트를 사용하고, 계정 잔액이 임대료 면제 최소값 아래로 떨어지는 것을 방지하여 임대료 면제를 시행합니다. 이 확인의 예외는 최종 잔액이 제로 램프 포트가되어 계정을 삭제할 수있는 경우입니다. 이 계정 폐쇄 세부 사항에는 저장된 nonce 값이`AdvanceNonceAccount`에 따라 클러스터의 가장 최근 블록 해시와 일치하지 않아야한다는 추가 요구 사항이 있습니다.

계정의 \[nonce Authority\] (../ offline-signing / durable-nonce.md # nonce-authority)는`AuthorizeNonceAccount` 명령어를 사용하여 변경할 수 있습니다. 새 권한의 'Pubkey'라는 하나의 매개 변수를 사용합니다. 이 명령을 실행하면 새 권한에 계정과 잔액에 대한 모든 권한이 부여됩니다.

> `AdvanceNonceAccount`,`WithdrawNonceAccount` 및`AuthorizeNonceAccount`는 모두 계정이 거래에 서명하기 위해 현재 \[nonce Authority\] (../ offline-signing / durable-nonce.md # nonce-authority)를 필요로합니다.

### 런타임 지원

컨트랙트만으로는이 기능을 구현하는 데 충분하지 않습니다. 트랜잭션에 현존하는 'recent_blockhash'를 적용하고 실패한 트랜잭션 재생을 통한 수수료 도난을 방지하려면 런타임 수정이 필요합니다.

일반적인`check_hash_age` 유효성 검사에 실패한 모든 트랜잭션은 Durable Transaction Nonce에 대해 테스트됩니다. 이것은 트랜잭션의 첫 번째 명령으로`AdvanceNonceAccount` 명령을 포함함으로써 알 수 있습니다.

런타임에서 Durable Transaction Nonce가 사용을 확인하면 트랜잭션을 확인하기 위해 다음과 같은 추가 작업을 수행합니다

1. 중임. 1.`Nonce` 명령에 지정된`NonceAccount`가로드됩니다.
2. 2.`NonceState`는`NonceAccount`의 데이터 필드에서 역 직렬화되고`Initialized` 상태임을 확인합니다.
3. 3.`NonceAccount`에 저장된 nonce 값이 트랜잭션의`recent_blockhash` 필드에 지정된 값과 일치하는지 테스트합니다.

위의 세 가지 검사가 모두 성공하면 트랜잭션이 유효성 검사를 계속할 수 있습니다.

'InstructionError'로 실패한 거래에는 수수료가 부과되고 상태가 롤백되기 때문에 'AdvanceNonceAccount'명령어가 되 돌리면 수수료 도난의 기회가 있습니다. 악의적 인 밸리데이터는 저장된 nonce가 성공적으로 진행될 때까지 실패한 트랜잭션을 재생할 수 있습니다. 런타임 변경으로 인해이 동작이 방지됩니다. 영구 nonce 트랜잭션이`AdvanceNonceAccount` 명령을 제외하고`InstructionError`로 실패하면 nonce 계정은 평소와 같이 사전 실행 상태로 롤백됩니다. 그런 다음 런타임은 성공한 것처럼 nonce 값과 저장된 고급 nonce 계정을 진행합니다.
