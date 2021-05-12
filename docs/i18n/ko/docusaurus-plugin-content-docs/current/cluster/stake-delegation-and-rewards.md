---
title: 스테이킹 위임 및 보상
---

스테이커는 원장을 검증하는 데 도움을 준 것에 대한 보상을 받습니다. 스테이커들은 지분을 밸리데이터 노드에 위임함으로써 이를 수행합니다. 이러한 밸리데이터는 원장을 재구현하고 스테이커가 지분을 위임할 수 있는 노드별 투표 계정으로 투표를 보내는 작업을 수행합니다. 나머지 클러스터는 포크가 발생할 시 블록을 선택하기 위해 이러한 지분 가중치 투표를 사용합니다. 벨리데이터와 스테이커 모두 자신의 역할을 수행하려면 경제적 인센티브가 필요합니다. 밸리데이터는 하드웨어에 대한 보상을 받아야하며 스테이커는 지분이 삭감될 위험에 대한 보상을 받아야합니다. 이에 대한 경제구조는 [스테이킹 보상](../implemented-proposals/staking-rewards.md)에서 다룹니다. 해당 섹션에서는 구현의 기본 메커니즘에 대해 설명합니다.

## 기본 디자인

일반적인 아이디어는 밸리데이터가 투표 계정을 소유한다는 것입니다. 투표 계정은 밸리데이터 투표를 추적하고 밸리데이터가 생성한 크레딧을 계산하며 밸리데이터의 추가적인 특정 상태를 제공합니다. 투표 계정은 자신에게 위임된 지분을 인식하지 못하며 스테이킹 가중치가 없습니다.

별도의 스테이킹 계정 \(스테이커가 생성\) 은 스테이킹이 위임된 투표 계정의 이름을 지정합니다. 생성된 보상은 스테이킹 된 램포트 양에 비례합니다. 스테이킹 계정은 스테이커 만 소유합니다. 이 계정에 저장된 램포트의 일부는 지분입니다.

## 정적 위임

투표 계정을 제어하거나 계정에 투표를 제출하는 존재의 상호작용 없이도 모든 스테이킹 계정을 단일 투표 계정에 위임할 수 있습니다.

투표 계정에 할당 된 총 지분은 `StakeState::Stake::voter_pubkey` 로 투표 계정 pubkey가 있는 모든 스테이킹 계정의 합계로 계산할 수 있습니다.

## 투표 및 스테이킹 계정

보상 프로세스는 두 개의 온 체인 프로그램으로 나뉩니다. 투표 프로그램은 지분을 삭감 할 수있는 문제를 해결합니다. 스테이킹 프로그램은 보상 풀의 관리자 역할을하며 정적 위임을 제공합니다. 스테이킹 프로그램은 스테이커의 대리인이 원장 검증에 참여했음을 보여줄 때 스테이커와 투표자에게 보상을 지급할 책임이 있습니다.

### VoteState

VoteState는 밸리데이터가 네트워크에 제출한 모든 투표의 현재 상태입니다. VoteState는 다음 상태 정보를 포함합니다:

- `votes`-제출된 투표 데이터 구조.
- `credits`-해당 투표 프로그램이 평생 동안 생성한 총 보상 수입니다.
- `root_slot`-보상에 필요한 전체 락업 약정에 도달하는 마지막 슬롯입니다.
- `commission`-스테이커의 스테이킹 계정에 의해 청구된 보상에 대해 VoteState가 취하는 수수료. 이것은 보상의 백분율 한도입니다.
- Account::lamports -수수료로부터 축적된 램포트. 이는 스테이킹으로 간주되지 않습니다.
- `authorized_voter`-해당 계정만이 투표를 제출할 수 있습니다. 이 필드는해당 계정으로만 수정할 수 있습니다.
- `node_pubkey` - 해당 계정에 투표한 솔라나 계정.
- `authorized_withdrawer` - 계정의 주소 및 승인된 투표 서명자와 별도로 이 계정의 램포트를 담당하는 계정.

### VoteInstruction::Initialize\(VoteInit\)

- `account[0]` - RW - The VoteState.

  `VoteInit` 는 신규 투표계정의 `node_pubkey`, `authorized_voter`, `authorized_withdrawer`, 그리고`commission`를 지닙니다.

  다른 VoteState 멤버들은 탈락합니다.

### VoteInstruction::Authorize\(Pubkey, VoteAuthorize\)

VoteAuthorize 매개 변수\(`Voter` or `Withdrawer`\) 에 따라 새롭게 승인된 투표자 또는 인출자로 계정을 업데이트 합니다. 트랜잭션은 투표 계정의 현재`authorized_voter` 또는`authorized_withdrawer`가 서명해야합니다.

- `account[0]` - RW - The VoteState. `VoteState::authorized_voter` 또는`authorized_withdrawer`가`Pubkey`로 설정되어 있습니다.

### VoteInstruction::Vote\(Vote\)

- `account[0]` - RW - The VoteState. `VoteState::lockouts`와 `VoteState::credits` 투표 락아웃 규칙에 따라 업데이트 됩니다. [Tower BFT](../implemented-proposals/tower-bft.md)를 참고하세요.
- `account[1]` - RO - `sysvar::slot_hashes` 검증 할 투표에 대한 N 개의 최근 슬롯 및 해시 목록입니다.
- `account[2]` - RO - `sysvar::clock` 현재 네트워크 시간인 슬롯, 에폭 단위 입니다.

### StakeState

StakeState는 takeState::Uninitialized, StakeState::Initialized, StakeState::Stake, and StakeState::RewardsPool의 네 가지 형식 중 하나를 사용합니다. 처음 세 가지 형식만 스테이킹에 사용되지만 StakeState::Stake는 특별합니다. 모든 RewardsPools는 제네시스에 생성됩니다.

### StakeState::Stake

StakeState::Stake는 **staker**에 대한 현재 우선적인 위임 대상이며 다음과 같은 상태 정보를 담고 있습니다.

- Account::lamports - 스테이킹 가능한 램포트
- `stake`- 보상 생성을 위한 스테이킹 금액 \(워밍업 및 쿨다운 대상\), 항상 Account::lamports보다 작거나 같습니다.
- `voter_pubkey`- 램포트가 위임된 VoteState 인스턴스의 pubkey입니다.
- `credits_observed` - 프로그램의 전체 클레임 크레딧 입니다.
- `activated` - 스테이킹이 활성화/위임된 에폭. 워밍엄 이후 전체 스테이킹이 인식됩니다.
- `deactivated` - 스테이킹이 비활성화된 에폭이며, 몇몇 계정의 비활성화 및 출금은 쿨다운을 위한 에폭을 필요로 합니다.
- `authorized_staker`- 위임, 활성화 및 비활성화 트랜잭션에 서명해야하는 엔티티의 pubkey입니다.
- `authorized_withdrawer`- 계정 주소와 별도로 이 계정의 램포트를 담당하는 주체의 신원 및 승인된 스테이커입니다.

### StakeState::RewardsPool

단일 네트워크 규모 락이나 구제에 대한 불화를 회피하기 위해 256 RewardsPools는 미리 정해진 키 아래의 제네시스을 일부로서 되어있으며, 각 std::u64::MAX 크레딧을 가지며 포인트 값에 따른 구제를 충족합니다.

Stakes와 RewardsPool은 동일한 `Stake`프로그램이 소유 한 계정입니다.

### StakeInstruction::DelegateStake

Stake 계정은 Initialized에서 StakeState::Stake 형태, 또는 비활성화된 (i.e. 완전 쿨다운화) StakeState::Stake에서 활성화된 activated StakeState::Stake로 넘어가게 됩니다. 이것이 스테이커가 자신의 스테이킹 계정 램프트가 위임되는 투표 계정과 밸리데이터 노드를 선택하는 방법입니다. 거래는 스테이킹의`authorized_staker`에 의해 서명되어야합니다.

- `account [0]`-RW-StakeState::Stake 인스턴스. `StakeState::Stake::credits_observed`는 `VoteState::credits`로 초기화되며, `StakeState::Stake::voter_pubkey`는 `account[1]`로 초기화 됩니다. 만일 이것이 첫 위임이라면, `StakeState::Stake::stake`는 램포트의 계정 잔액으로 초기화되고, `StakeState::Stake::activated` 는 현 뱅크 에폭으로, `StakeState::Stake::deactivated`는 std::u64::MAX로 초기화 됩니다.
- `account[1]` - R - VoteState 인스턴스 입니다.
- `account[2]` - R - sysvar::clock account은 현 뱅크 에폭에 대한 정보를 가집니다.
- `account[3]` - R - sysvar::stakehistory account는 스테이킹 내역 정보를 가집니다.
- `account[4]` - R - stake::Config account는 워밍업, 쿨다운, 슬래싱 설정을 지닙니다.

### StakeInstruction::Authorize\(Pubkey, StakeAuthorize\)

StakeAuthorize 매개 변수 \ (`Staker` 또는`Withdrawer` \) 에 따라 새로운 승인된 스테이커 또는 인출자로 계정을 업데이트합니다. 트랜잭션은 스테이커 계정의 현재`authorized_staker` 또는`authorized_withdrawer`의 서명이 있어야 합니다. 모든 스테이킹 락업이 만료되었거나 락업 관리자도 트랜잭션에 서명해야합니다.

- `account[0]` - RW - The StakeState.

  `StakeState::authorized_staker` 또는 `authorized_withdrawer`는 `Pubkey`로 세팅됩니다.

### StakeInstruction::Deactivate

스테이커는 네트워크에서 철회를 원할 수 있습니다. 그렇게 하려면 먼저 스테이킹를 비활성화하고 쿨다운을 기다려야합니다. 트랜잭션은 스테이킹의`authorized_staker`에 의해 서명되어야 합니다.

- `account [0]`- RW - 비활성화중인 StakeState::Stake 인스턴스입니다.
- `account [1]` - R - sysvar::clock 현 에폭을 가지는 뱅크의 시간 계정 입니다.

StakeState::Stake::deactivated는 현 에폭 + 쿨다운으로 세팅됩니다. 계정의 지분은 해당 에폭에서 0으로 내려가며 Account::lamports는 출금될 수 있게 됩니다.

### StakeInstruction::Withdraw\(u64\)

램포트는 Stake 계정에 시간이 지남에 따라 축적되며 초과 활성화된 지분은 인출될 수 있습니다. 트랜잭션은 스테이킹의`authorized_withdrawer`에 의해 서명되어야합니다.

- `account [0]` - RW - 인출할 StakeState::Stake.
- `account [1]` - RW - 인출된 램프가 적립되어야 하는 계정입니다.
- `account [2]` - R - sysvar::clock 뱅크의 시간 계정이며 스테이킹 계산을 위해 현 에폭을 지니고 있습니다.
- `account [3]` - R - sysvar::stake_history 스테이킹 워밍업/쿨다운 기록을 전달하는 내역계정.

## 디자인의 이점

- 모든 스테이커들은 단일 투표를 지닙니다.
- -보상을 청구하기 위해 크레딧 변수를 지울 필요가 없습니다.
- 각 위임된 지분은 독립적으로 보상을 수령할 수 있습니다.
- 위임된 지분에 대한 보상 수령 시 수수료가 적립됩니다.

## Callflow 예시

![수동적 스테이킹 Callflow](/img/passive-staking-callflow.png)

## 스테이킹 보상

밸리데이터 보상 제도의 구체적인 메커니즘과 규칙에 대한 설명입니다. 보상은 올바르게 투표하는 밸리데이터에게 지분을 위임하여 얻을 수 있습니다. 잘못된 투표는 밸리데이터의 지분이 [slashing](../proposals/slashing.md) 위험에 노출되게 합니다.

### 기본

네트워크는 네트워크의 [inflation](../terminology.md#inflation) 일부에서 보상을 지급합니다. 한 에폭에 대한 보상을 지불하는 데 사용할 수있는 램포트의 수는 고정되어 있으며 상대적 지분 가중치 및 참여도에 따라 모든 지분 노드간에 균등하게 분배되어야합니다. 가중치 단위를[point](../terminology.md#point)라고합니다.

에폭 보상은 해당 에폭 종료까진 수령할 수 없습니다.

각 에폭이 끝날 때 에폭 동안 획득한 총 포인트 수를 합산하여 에폭 인플레이션의 보상 부분을 나누어 포인트 값에 도달합니다. 이 값은 에포크를 포인트 값에 매핑하는[sysvar](../terminology.md#sysvar)의 뱅크에 기록됩니다.

구제 기간동안 스테이킹 프로그램은 각 에폭마다 스테이커가 벌어들인 포인트를 계산하고, 이를 에폭의 포인트 값에 곱합니다. 그리고 보상 계정에서 램포트를 해당 스테이킹 계정과 투표 계정으로 이체하며 투표 계정의 수수료 세팅에 따라 받게 됩니다.

### 경제성

한 에폭에 대한 포인트 값은 총 네트워크 참여도에 따라 달라집니다. 에포크 참여가 떨어지면 참여한 사람들의 점수가 높아집니다.

### 크레딧 획득

밸리데이터는 최대 락업을 초과하는 모든 올바른 투표에 대해 하나의 투표 크레딧을 얻습니다. 즉, 밸리데이터의 투표 계정이 럭옵 목록에서 슬롯을 폐기 할 때마다 해당 투표가 노드의 루트가됩니다.

해당 밸리데이터에게 위임한 스테이커는 지분에 비례하여 포인트를 얻습니다. 획득한 포인트는 투표 크레딧과 지분의 산물입니다.

### 스테이킹 워밍업, 쿨다운, 인출

스테이킹은 일단 위임되면 즉시 효력이 발생하지 않습니다. 먼저 워밍업 기간을 거쳐야합니다. 이 기간 동안 지분의 일부는 "유효"로 간주되고 나머지는 "활성화"로 간주됩니다. 에폭의 경계에서 변경이 발생합니다.

스테이킹 프로그램은 스테이킹 프로그램의`config::warmup_rate` \(현재 에폭 당 25%로 세팅\)에 반영된 전체 네트워크 지분에 대한 변경 비율을 제한합니다.

각 에폭을 워밍업 할 수 있는 지분의 양은 이전 에폭의 총 유효 지분, 총 활성화 지분 및 지분 프로그램의 구성된 워밍업 비율의 함수입니다.

쿨다운은 같은 방식으로 작동합니다. 스테이킹이 비활성화되면 일부는 "유효"하고 "비활성화"된 것으로 간주됩니다. 스테이킹 쿨다운이 완료되면 계속해서 보상을 얻고 슬래싱에 노출되지만 인출도 가능합니다.

부트스트랩 스테이킹은 워밍업 대상이 아닙니다.

보상은 해당 에폭에 대한 지분의 "유효"부분에 대해 지급됩니다.

#### 워밍업 예시

네트워크 워밍업 비율이 20% 인 N 에폭에 활성화된 단일 지분 1,000개와 에폭 N에서 정지된 총 네트워크 지분이 2,000 인 상황을 생각해봅시다.

N + 1 에폭에서 네트워크의 활성화에 가용될 수 있는 수는 400 입니다 \(2000의 20%\). N 에폭에선 이는 활성화된 스테이킹이며 워밍업이 될 수 있습니다.

| 에폭    |   가용 |   활성화 |  총 가용 | 총 활성화 |
|:----- | ----:| -----:| -----:| -----:|
| N-1   |      |       | 2,000 |     0 |
| N     |    0 | 1,000 | 2,000 | 1,000 |
| N + 1 |  400 |   600 | 2,400 |   600 |
| N + 2 |  880 |   120 | 2,880 |   120 |
| N + 3 | 1000 |     0 | 3,000 |     0 |

두 개의 스테이킹 \(X 및 Y \) 이 N 에폭에 활성화 되었으면 지분에 비례하여 20%의 일부를 받게됩니다. 각 에폭에서 각 스테이킹에 대해 유효하고 활성화 된 것은 이전 에폭 상태의 기능입니다.

| 에폭    | X eff | X act | Y eff | Y act |  총 가용 | 총 활성화 |
|:----- | -----:| -----:| -----:| -----:| -----:| -----:|
| N-1   |       |       |       |       | 2,000 |     0 |
| N     |     0 | 1,000 |     0 |   200 | 2,000 | 1,200 |
| N + 1 |   333 |   667 |    67 |   133 | 2,400 |   800 |
| N + 2 |   733 |   267 |   146 |    54 | 2,880 |   321 |
| N + 3 |  1000 |     0 |   200 |     0 | 3,200 |     0 |

### 철회

유효 + 활성화 지분을 초과하는 램포트는 언제든지 철회할 수 있습니다. 이는 즉 워밍업 동안 어떠한 지분도 철회가 불가능하다는 뜻입니다. 쿨다운 동안 유효 지분을 초과하는 모든 토큰은 인출될 수 있습니다. \(activating == 0\). 획득한 보상은 자동으로 스테이킹에 추가되기 때문에 인출은 일반적으로 비활성화 후에만 ​​가능합니다.

### 락업

락업 스테이킹 계정은 락업이라는 개념을 지원하며, 스테이킹 계정 잔액은 지정된 시간까지 인출할 수 없습니다. 락업은 에폭 높이로 지정됩니다. 즉, 특정 관리자가 거래에 서명하지 않는한 스테이킹 계정 잔액을 인출할 수 있기 전에 네트워크가 도달해야하는 최소 에폭 높이입니다. 이 정보는 스테이킹 계정이 생성될 때 수집되어 스테이킹 계정 상태의 락업 필드에 저장됩니다. 인증된 스테이커 또는 인출자 변경은 락업의 대상이며, 그렇기에 실질적으로 이는 이체 행위 입니다.
