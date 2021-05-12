---
title: Validator Timestamp Oracle
---

Solana의 타사 사용자는 일반적으로 외부 감사 자 또는 법 집행 기관에 대한 규정 준수 요구 사항을 충족하기 위해 블록이 생성 된 실제 시간을 알아야하는 경우가 있습니다. 이 제안은 Solana 클러스터가 이러한 요구를 충족시킬 수있는 밸리데이터 타임스탬프 오라클을 설명합니다.

제안 된 구현의 일반적인 개요는 다음과 같습니다

- .-정기적으로 각 밸리데이터은 알려진 슬롯 온 체인에 대해 관찰 된 시간을 기록합니다 (슬롯 투표에 추가 된 타임스탬프를 통해) -클라이언트는 루트에 대한 블록 시간을 요청할 수 있습니다.
- A client can request a block time for a rooted block using the `getBlockTime` RPC method. 클라이언트가 블록 N에 대한 타임스탬프를 요청하는 경우 :

  1. 2.이 최근 평균 타임스탬프는 클러스터의 설정된 슬롯 기간을 사용하여 블록 N의 타임스탬프를 계산하는 데 사용됩니다

  2. This recent mean timestamp is then used to calculate the timestamp of block N using the cluster's established slot duration

요구 사항 :

- -미래에 원장을 재생하는 모든 밸리데이터은 생성 이후 모든 블록에 대해 동일한 시간을 가져와야합니다 .-예상 블록 시간은 다음과 같아야합니다.
- Estimated block times should not drift more than an hour or so before resolving to real-world (oracle) data
- 실제 (오라클) 데이터로 분석하기 전에 한 시간 이상 드리프트하지 마십시오 .-블록 시간은 단일 중앙 오라클에 의해 제어되지 않지만 이상적으로는 모든 밸리데이터의 입력을 사용하는 기능을 기반으로합니다 .-각 밸리데이터은 타임스탬프를 유지해야합니다. oracle
- Each validator must maintain a timestamp oracle

동일한 구현은 아직 루팅되지 않은 블록에 대한 타임스탬프 추정치를 제공 할 수 있습니다. 그러나 가장 최근의 타임스탬프 슬롯이 아직 루팅 될 수도 있고 그렇지 않을 수도 있기 때문에이 타임스탬프는 불안정 할 수 있습니다 (요구 사항 1이 실패 할 수 있음). 초기 구현은 루팅 된 블록을 대상으로하지만 최근 블록 타임스탬프에 대한 사용 사례가있는 경우 향후 RPC API를 추가하는 것은 간단합니다.

## 기록 시간

특정 슬롯에서 투표 할 때 정기적으로 각 밸리데이터는 투표 명령 제출에 타임스탬프를 포함하여 관찰 된 시간을 기록합니다. 타임스탬프에 해당하는 슬롯은 투표 벡터 (`Vote :: slots.iter (). max ()`)의 최신 슬롯입니다. 일반 투표로 밸리데이터의 신원 키 쌍에 의해 서명됩니다. 이보고 기능을 사용하려면 대부분의 투표에서 '없음'으로 설정되는 타임스탬프 필드`timestamp : Option <UnixTimestamp>`를 포함하도록 Vote 구조체를 확장해야합니다.

https://github.com/solana-labs/solana/pull/10630부터 밸리데이터은 투표 할 때마다 타임스탬프를 제출합니다. 이를 통해 노드가 블록이 루팅 된 직후 예상 타임스탬프를 계산하고 해당 값을 Blockstore에 캐시 할 수있는 블록 시간 캐싱 서비스를 구현할 수 있습니다. 이는 위의 요구 사항 1)을 충족하면서 지속적인 데이터와 빠른 쿼리를 제공합니다.

### 투표 계정

A validator's vote account will hold its most recent slot-timestamp in VoteState.

### 투표 프로그램

온 체인 투표 프로그램은 밸리데이터의 투표 명령과 함께 전송 된 타임스탬프를 처리하기 위해 확장되어야합니다. 현재 process_vote 기능 (올바른 투표 계정로드 및 트랜잭션 서명자가 예상 유효성 검사자인지 확인 포함) 외에도이 프로세스는 타임스탬프와 해당 슬롯을 현재 저장된 값과 비교하여 둘 다 단조롭게 증가하는지 확인해야합니다.

## 스테이킹 - 가중 평균

밸리데이터의 투표 계정은 VoteState에서 가장 최근의 슬롯 타임스탬프를 보유합니다.

```text
let timestamp_slot = floor(current_slot / timestamp_interval);
```

그런 다음 밸리데이터는`Blockstore :: get_slot_entries ()`를 사용하여 해당 슬롯을 참조하는 원장에서 모든 Vote WithTimestamp 트랜잭션을 수집해야합니다. 이러한 트랜잭션은 리더가 도달하고 처리하는 데 약간의 시간이 소요될 수 있으므로 밸리데이터는 적절한 타임스탬프 세트를 얻기 위해 timestamp_slot 이후에 완료된 여러 블록을 스캔해야합니다. 슬롯이 많을수록 더 많은 클러스터 참여와 더 많은 타임스탬프 데이터 포인트가 가능합니다. 슬롯 수가 적을수록 타임스탬프 필터링에 걸리는 시간이 단축됩니다.

특정 블록에 대해 가장 최근에 타임스탬프 슬롯을 확인하기 위해, 검증이 먼저 요구 추정 타임스탬프를 계산하는소인에서순서를

계산``텍스트 timestamp_slot = 층 (current_slot / timestamp_interval)하자;

## Calculating Estimated Time for a Particular Block

여기서`block_n_slot_offset`은 블록 N의 슬롯과 timestamp_slot의 차이이고`slot_duration`은 각 뱅크에 저장된 클러스터의`slots_per_year`에서 파생됩니다.

```text
##는특정 블록에 대한 시간 기준

공지 슬롯 평균 소인을 산출하면  후속 블록 N에 대한 추정 된 타임스탬프를 계산하는 사소한,

계산``텍스트
block_n_timestamp = mean_timestamp + (block_n_slot_offset * slot_duration)을 보자;
```

where `block_n_slot_offset` is the difference between the slot of block N and the timestamp_slot, and `slot_duration` is derived from the cluster's `slots_per_year` stored in each Bank
