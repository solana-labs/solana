---
title: 단순 결제 및 상태 확인
---

리소스가 부족한 클라이언트가 Solana 클러스터에 참여하도록 허용하는 것이 종종 유용합니다. 이러한 참여 경제 또는 컨트랙트 실행에 따라 클라이언트의 활동이 네트워크에 의해 수락되었는지 확인하는 데 일반적으로 비용이 많이 듭니다. 이 제안은 그러한 고객이 최소한의 자원 지출과 제 3 자 신뢰로 자신의 행동이 원장 상태에 투입되었음을 확인할 수있는 메커니즘을 제시합니다.

## A Naive Approach

Validator는 최근 확인 된 거래의 서명을 짧은 기간 동안 저장하여 두 번 이상 처리되지 않도록합니다. 유효성 검사기는 트랜잭션이 최근에 처리 된 경우 클라이언트가 클러스터를 쿼리하는 데 사용할 수있는 JSON RPC 끝점을 제공합니다. 유효성 검사기는 또한 PubSub 알림을 제공하며,이를 통해 클라이언트는 유효성 검사자가 주어진 서명을 관찰 할 때 알림을 받도록 등록합니다. 이 두 가지 메커니즘은 클라이언트가 지불을 확인할 수 있도록 허용하지만 증거가 아니며 밸리데이터을 완전히 신뢰하는 데 의존합니다.

Merkle Proofs를 사용하여 밸리데이터의 응답을 원장에 고정함으로써이 신뢰를 최소화하는 방법을 설명하여 클라이언트가 선호하는 밸리데이터이 충분한 수의 트랜잭션을 확인했는지 스스로 확인할 수 있도록합니다. 여러 밸리데이터 증명을 요구하면 다른 여러 네트워크 참가자를 손상시키는 기술적 및 경제적 어려움이 증가하므로 밸리데이터에 대한 신뢰가 더욱 낮아집니다.

## 라이트 클라이언트

'라이트 클라이언트'는 자체적으로 유효성 검사기를 실행하지 않는 클러스터 참여자입니다. 이 라이트 클라이언트는 라이트 클라이언트가 원장을 확인하는 데 많은 리소스를 소비하지 않고도 원격 유효성 검사기를 신뢰하는 것보다 더 높은 수준의 보안을 제공합니다.

라이트 클라이언트에 직접 트랜잭션 서명을 제공하는 대신 유효성 검사기는 관심있는 트랜잭션에서 포함 블록에있는 모든 트랜잭션의 머클 트리 루트까지 머클 증명을 생성합니다. 이 Merkle Root는 밸리데이터가 투표 한 원장 항목에 저장되어 합의 합법성을 제공합니다. 라이트 클라이언트에 대한 추가 보안 수준은 라이트 클라이언트가 클러스터의 이해 관계자로 간주하는 초기 표준 유효성 검사기 집합에 따라 다릅니다. 해당 세트가 변경되면 클라이언트는 \[receipts\] (simple-payment-and-state-verification.md # receipts)로 알려진 내부 밸리데이터 세트를 업데이트 할 수 있습니다. 이것은 많은 수의 위임 스테이킹에서 어려울 수 있습니다.

유효성 검사기 자체가 성능상의 이유로 라이트 클라이언트 API를 사용할 수 있습니다. 예를 들어, 유효성 검사기의 초기 실행 중에 유효성 검사기는 클러스터에서 제공하는 상태 체크 포인트를 사용하여 영수증으로 확인할 수 있습니다.

## 영수증

영수증은 다음과 같은 최소한의 증거입니다. 트랜잭션이 블록에 포함되었고, 블록이 클라이언트가 선호하는 밸리데이터 세트에 의해 투표되었으며 투표가 원하는 확인 깊이에 도달했습니다.

### Transaction Inclusion Proof

트랜잭션 포함 증명은 트랜잭션에서 Entry-Merkle을 통해 Block-Merkle까지의 Merkle 경로를 포함하는 데이터 구조로, 필요한 밸리데이터 투표 세트와 함께 Bank-Hash에 포함됩니다. Bank-Hash에서 파생 된 후속 밸리데이터 투표를 포함하는 역사증명 항목 체인은 확인의 증거입니다.

#### Transaction Merkle

Entry-Merkle은 서명별로 정렬 된 지정된 항목의 모든 트랜잭션을 포함하는 Merkle Root입니다. 항목의 각 트랜잭션은 https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/ledger/src/entry.rs#L205에서 이미 머 클링되었습니다. 즉, 항목 'E'에 트랜잭션 'T'가 포함되었음을 표시 할 수 있습니다.

Block-Merkle은 블록에서 시퀀싱 된 모든 Entry-Merkles의 Merkle Root입니다.

![Block Merkle Diagram](/img/spv-block-merkle.svg)

두 개의 merkle 증명은 함께 트랜잭션`T`가 은행 해시`B`가있는 블록에 포함되었음을 보여줍니다.

계정-해시는 현재 슬롯 동안 수정 된 각 계정의 상태 해시 연결 해시입니다.

블록에 대한 상태 영수증이 구성되어 있기 때문에 영수증에 대한 거래 상태가 필요합니다. 동일한 상태에 대한 두 개의 트랜잭션이 블록에 나타날 수 있으므로 원장에 커밋 된 트랜잭션이 의도 한 상태를 수정하는 데 성공했는지 실패했는지 여부를 상태만으로 추론 할 방법이 없습니다. 전체 상태 코드를 인코딩 할 필요는 없지만 트랜잭션의 성공을 나타내는 단일 상태 비트를 인코딩 할 수 있습니다.

현재 Block-Merkle은 구현되어 있지 않으므로 'E'가 뱅크 해시 'B'가있는 블록의 항목인지 확인하려면 블록의 모든 항목 해시를 제공해야합니다. 대안이 매우 비효율적이기 때문에 이상적으로이 Block-Merkle은 구현 될 것입니다.

#### 블록 헤더

트랜잭션 포함 증명을 확인하기 위해 라이트 클라이언트는 네트워크에서 포크의 토폴로지를 추론 할 수 있어야합니다.

더 구체적으로, 라이트 클라이언트는 들어오는 블록 헤더를 추적하여 두 개의 은행 해시가 제공되도록해야합니다. 블록`A`와`B`는`A`가`B`의 조상인지 여부를 결정할 수 있습니다 (`낙관적 확인 증명`섹션에서 이유를 설명합니다!). 헤더의 내용은 은행 해시를 계산하는 데 필요한 필드입니다.

Bank-Hash는 위의 'Transaction Merkle'섹션에서 설명한 Block-Merkle과 Accounts-Hash를 연결 한 해시입니다.

![Bank Hash Diagram](/img/spv-bank-hash.svg)

코드에서 :

https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/runtime/src/bank.rs#L3468-

```
        L3473``상위
        및  해시 =
            블록의[//뱅크 해시
            self.parent_hash.as_ref((MUThashv하자)
            //모든 변형 한 계정  해시
            accounts_delta_hash.hash.as_ref()의//
            서명 번호를 처리 이 블록에서이 블록의
            & signature_count_buf,
            //마지막 역사증명 해시
            self.last_blockhash (). as_ref (),
        ]);
```

밸리데이터의 재생 로직에서 기존 스트리밍 로직과 함께이 로직을 구현하기에 좋은 곳 : https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/core/src/replay_stage.rs#L1092-L1096 # ### 낙관적 확인 증거 현재 낙관적 확인은 가십 및 투표 재생 파이프 라인을 모니터링하는 리스너를 통해 감지됩니다.

#### Optimistic Confirmation Proof

B-> B 'B'

각 투표는 밸리데이터가 투표 한 블록의 은행 해시를 포함하는 서명 된 트랜잭션입니다. 네트워크의 특정 임계 값 'T'가 블록에 투표하면 해당 블록은 낙관적으로 확인 된 것으로 간주됩니다. 이 'T'밸리데이터 그룹의 투표는 은행 해시 'B'가있는 블록이 낙관적으로 확인되었음을 보여주기 위해 필요합니다.

However other than some metadata, the signed votes themselves are not currently stored anywhere, so they can't be retrieved on demand. These votes probably need to be persisted in Rocksdb database, indexed by a key `(Slot, Hash, Pubkey)` which represents the slot of the vote, bank hash of the vote, and vote account pubkey responsible for the vote.

Together, the transaction merkle and optimistic confirmation proofs can be provided over RPC to subscribers by extending the existing signature subscrption logic. Clients who subscribe to the "SingleGossip" confirmation level are already notified when optimistic confirmation is detected, a flag can be provided to signal the two proofs above should also be returned.

합성 상태는 뱅크 생성 상태와 함께 Bank-Hash로 계산되어야합니다.

```

B -> B'

```

위의 예에서 블록`가 낙관적으로 확인되면`B`도 그렇습니다. 따라서 트랜잭션이 블록 'B'에 있었다면 증명의 트랜잭션 머클은 블록 'B'에 대한 것이지만 증명에 제시된 투표는 블록 'B'에 대한 것입니다. 이것이 위의`Block headers`섹션의 헤더가 중요한 이유입니다. 클라이언트는`B`가 실제로`B'`의 조상인지 확인해야합니다.

#### Proof of Stake Distribution

지분 분배 증명 위의 거래 머클 및 낙관적 확인 증명이 제시되면 고객은 은행 해시`B`가있는 블록에서 거래`T`가 낙관적으로 확인되었음을 확인할 수 있습니다. 마지막 누락 된 부분은 위의 낙관적 증명에서 투표가 실제로 "낙관적 확인"의 안전 보장을 유지하는 데 필요한 지분의 유효한 'T'비율을 구성하는지 확인하는 방법입니다.

One way to approach this might be for every epoch, when the stake set changes, to write all the stakes to a system account, and then have validators subscribe to that system account. Full nodes can then provide a merkle proving that the system account state was updated in some block `B`, and then show that the block `B` was optimistically confirmed/rooted.

### 검증

An account's state (balance or other data) can be verified by submitting a transaction with a **_TBD_** Instruction to the cluster. The client can then use a [Transaction Inclusion Proof](#transaction-inclusion-proof) to verify whether the cluster agrees that the acount has reached the expected state.

### 합성 상태

Leaders should coalesce the validator votes by stake weight into a single entry. This will reduce the number of entries necessary to create a receipt.

### Chain of Entries

A receipt has a 역사증명 link from the payment or state Merkle Path root to a list of consecutive validation votes.

It contains the following:

- Transaction -&gt; Entry-Merkle -&gt; Block-Merkle -&gt; Bank-Hash

라이트 항목은 항목에서 재구성되며 전체 트랜잭션 세트 대신 역사증명 해시에 혼합 된 항목 Merkle Root를 표시합니다.

- Validator vote entries
- Ticks
- Light entries

```text
/// This Entry definition skips over the transactions and only contains the
/// hash of the transactions used to modify 역사증명.
LightEntry {
    /// The number of hashes since the previous Entry ID.
    pub num_hashes: u64,
    /// The SHA-256 hash `num_hashes` after the previous Entry ID.
    hash: Hash,
    /// The Merkle Root of the transactions encoded into the Entry.
    entry_hash: Hash,
}
```

라이트 항목은 항목에서 재구성되며 전체 트랜잭션 세트 대신 PoH 해시에 혼합 된 항목 Merkle Root를 표시합니다.

클라이언트는 투표 시작 상태가 필요하지 않습니다. \[fork selection\] (../ implemented-proposals / tower-bft.md) 알고리즘은 트랜잭션 후 나타나는 투표 만 트랜잭션에 대한 최종성을 제공하고 최종성은 시작 상태와 무관하도록 정의됩니다.

### Verification

상위 다수 세트 밸리데이터를 알고있는 라이트 클라이언트는 역사증명 체인에 대한 Merkle 경로를 따라 영수증을 검증 할 수 있습니다. Block-Merkle은 Merkle Root이며 항목에 포함 된 투표에 나타납니다. 라이트 클라이언트는 연속 투표에 대해 \[포크 선택\] (../ implemented-proposals / tower-bft.md)을 시뮬레이션하고 원하는 잠금 임계 값에서 영수증이 확인되었는지 확인할 수 있습니다.

### Synthetic State

예 :

예 :

- -Epoch 유효성 검사기 계정과 지분 및 가중치.
- Computed fee rates

이 값은 Bank-Hash에 항목이 있어야합니다. 알려진 계정 아래에 있어야하므로 해시 연결에 대한 색인이 있어야합니다.
