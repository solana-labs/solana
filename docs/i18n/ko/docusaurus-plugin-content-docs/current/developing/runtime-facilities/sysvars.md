---
title: Sysvar 클러스터 데이터
---

솔라나는 [`sysvar`](terminology.md#sysvar) 계정을 통해 다양한 클러스터 상태 데이터를 프로그램에게 보여줍니다. 이러한 계정들은 [`solana-program` crate](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html) 레이아웃 계정들을 비롯하여 알려진 주소들 사이에 존재하며 아래에 나와있습니다.

sysvar 데이터를 프로그램 연산에 넣기 위해선 트랜잭션 내 계정 리스트의 sysvar 계정 주소들을 넘겨야 합니다. 계정은 여러분의 명령 프로세스에서 읽어볼 수 있습니다. Access to sysvars accounts is always _readonly_.

## Clock

Clock sysvar는 현재 슬롯, 에폭, 그리고 예상 유닉스 타임스탬프 등을 포함한 클러스터 시간을 담고 있습니다. 이는 매 슬롯마다 업데이트 됩니다.

- 주소: `SysvarC1ock11111111111111111111111111111111`
- 레이아웃: [Clock](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- 필드:

  - `slot`: 현재 슬롯
  - `epoch_start_timestamp`: 현 에폭 첫 슬롯의 유닉스 타임스탬프. 에폭의 첫 슬롯 타임스탭프는 `unix_timestamp`과 동일합니다 (아래).
  - `epoch`: 현 에폭
  - `leader_schedule_epoch`: 이미 생성된 리더 스케쥴에 대한 가장 최근의 에폭
  - `unix_timestamp`: 해당 슬롯에 대한 유닉스 타임스탬프

  각 슬롯은 역사증명에 기반한 예상 지속시간을 지닙니다. 하지만 현실적으로 슬롯의 시간은 예상시간보다 더 빠르거나 느리게 흐를 수 있습니다. 때문에 슬롯의 유닉스 타임스탬프는 투표하는 밸리데이터로부터 오는 입력값 오라클에 기반하여 생성됩니다. 타임스탬프는 투표를 통해 나온 타임스탬프 예상치 중 스테이킹 가중치의 중간값으로 산출되며, 에폭의 시작부터 흐른 예상 시간 범위 내에서 나옵니다.

  더욱 명확히 말하자면: 현 슬롯의 예상 타임스탬프 생성엔 각 밸리데이터로부터 나오는 가장 최근 투표 타임스탬프가 활용됩니다 (투표 타임스탬프로부터 흐른 시간은 Bank::ns_per_slot로 가정합니다). 각 예상 타임스탬프는 스테이킹에 따른 타임스탬프 분포를 생성하기 위해 투표 계정에 위임된 스테이킹과 연관되어 있습니다. `epoch_start_timestamp`로 부터 흐른 시간이 예상된 시간 흐름보다 25% 외로 이탈하지 않는다면 중간값의 타임스탬프를 `unix_timestamp`로 사용합니다.

## EpochSchedule

The EpochSchedule sysvar contains epoch scheduling constants that are set in genesis, and enables calculating the number of slots in a given epoch, the epoch for a given slot, etc. (Note: the epoch schedule is distinct from the [`leader schedule`](terminology.md#leader-schedule))

- 주소: `SysvarEpochSchedu1e111111111111111111111111`
- 레이아웃: [EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## Fees

Fees sysvar은 현 슬롯에 대한 수수료 계산기를 포함합니다. 이는 fee-rate governor에 기반하여 매 슬롯마다 업데이트 됩니다.

- 주소: `SysvarFees111111111111111111111111111111111`
- 레이아웃: [Fees](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## Instructions

Instructions sysvar엔 연속적 명령이 Message에 담겨있으며, 해당 Message가 처리되는 중에도 포함될 수 있습니다. 이는 프로그램 명령이 동일한 트랜잭션에 있는 다른 명령을 참고할 수 있도록 해줍니다. [instruction introspection](implemented-proposals/instruction_introspection.md)에서 더 자세히 알아보세요.

- 주소: `Sysvar1nstructions1111111111111111111111111`
- 레이아웃: [Instructions](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/struct.Instructions.html)

## RecentBlockhashes

RecentBlockhashes sysvar은 활성화된 최근 블록해시들과 이와 연관된 수수료 계산기를 내포하고 있습니다. 이는 매 슬롯마다 업데이트 됩니다.

- Address: `SysvarRecentB1ockHashes11111111111111111111`
- 레이아웃: [RecentBlockhashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## Rent

Rent sysvar은 대여료를 포함합니다. 현재 비율은 제네시스로부터 고정된 수치입니다. Rent 소각 비율은 수동 기능 활성화를 통해 변경됩니다.

- 주소: `SysvarRent111111111111111111111111111111111`
- 레이아웃: [Rent](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## SlotHashes

SlotHashes sysvar은 슬롯의 부모 bank의 가장 최근 해시를 담고 있습니다. 이는 매 슬롯마다 업데이트 됩니다.

- 주소: `SysvarS1otHashes111111111111111111111111111`
- 레이아웃: [SlotHashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## SlotHistory

SlotHistory sysvar은 지난 에폭에 존재하는 슬롯들의 이중벡터를 포함합니다. 이는 매 슬롯마다 업데이트 됩니다.

- 주소: `SysvarS1otHistory11111111111111111111111111`
- 레이아웃: [SlotHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## StakeHistory

StakeHistory sysvar은 에폭 당 전체 클러스터의 활성화 및 비활성화된 지분 내역을 담고 있습니다. 이는 매 에폭 시작마다 업데이트 됩니다.

- 주소: `SysvarStakeHistory1111111111111111111111111`
- 레이아웃: [StakeHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)
