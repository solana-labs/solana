---
title: 인플레이션 일정
---

**\*\*** 변경 될 수 있습니다. Solana 포럼에서 가장 최근의 경제 토론을 따르십시오 : https : //forums.solana.com**\*\***

Validator-client는 Solana 네트워크에서 두 가지 기능적 역할을합니다.

- 인플레이션 일정 \*이 Solana 경제에 미치는 영향을 이해하기위한 첫 번째 단계로 현재 연구중인 인플레이션 일정 매개 변수의 범위를 고려하여 시간 경과에 따른 토큰 발행의 상한 및 하한 범위를 시뮬레이션했습니다.
- -지분 가중 라운드 로빈 일정에서 '리더'로 선출되어 미결 거래를 수집하고이를 관찰 된 역사증명에 통합하여 네트워크의 글로벌 상태를 업데이트하고 체인 연속성을 제공합니다.

이러한 서비스에 대한 밸리데이터-클라이언트 보상은 각 Solana 시대가 끝날 때 배포됩니다. 앞서 논의한 바와 같이, 밸리데이터 고객에 대한 보상은 각 밸리데이터 노드의 지분 가중치에 비례하여 분산 된 프로토콜 기반 연간 인플레이션 율에 대해 부과되는 수수료를 통해 제공됩니다. 각 리더 로테이션 동안. 즉 주어진 밸리데이터-클라이언트가 리더로 선출되는 동안 각 거래 수수료의 일부를 유지하고 폐기되는 프로토콜 지정 금액을 뺀 \ (\[Validation-client State Transaction Fees\] (ed_vce_state_validation_transaction_fees.md 참조) ) \).

The effective protocol-based annual staking yield \(%\) per epoch received by validation-clients is to be a function of:

- the current global inflation rate, derived from the pre-determined dis-inflationary issuance schedule \(see [Validation-client Economics](ed_vce_overview.md)\)
- the fraction of staked SOLs out of the current total circulating supply,
- the commission charged by the validation service,
- the up-time/participation \[% of available slots that validator had opportunity to vote on\] of a given validator over the previous epoch.

유효성 검사 클라이언트가받는 효과적인 프로토콜 기반 연간 스테이킹 수익률 \ (% \)은 다음과 같습니다.

-사전 결정된 디스인플레이션 발행 ​​일정에서 도출 된 현재 글로벌 인플레이션 율 \ (\[Validation-client Economics\] (ed_vce_overview.md) \ 참조) -현재 총 순환 공급량 중 스테이킹 된 SOL의 비율, -검증 서비스에서 부과하는 수수료, -이전 에포크 동안 주어진 밸리데이터의 가동 시간 / 참가 \ [밸리데이터이 투표 할 기회가 있었던 사용 가능한 슬롯의 % \].

첫 번째 요소는 프로토콜 매개 변수의 기능으로 \ (즉, 주어진 시대의 밸리데이터 행동과는 무관 함 \) 조기 참여를 장려하고 명확한 통화 안정성을 제공하며 네트워크에서 최적의 보안을 제공하도록 설계된 인플레이션 일정을 생성합니다.

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

구체적으로 특별히:

![](/img/p_inflation_schedule_ranges_w_comments.png)

위의 그래프에서 범위의 평균 값은 각 매개 변수의 기여도를 보여주기 위해 식별됩니다. 시뮬레이션 된 * 인플레이션 일정 *에서 시간에 따른 토큰 발행 범위를 계획 할 수도 있습니다.

![](/img/p_total_supply_ranges.png)

마지막으로 앞서 논의한 추가 매개 변수 인 * % of Staked SOL *을 도입하면 스테이킹 된 SOL에 대한 * Staked Yield *를 추정 할 수 있습니다.

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

이 경우 * % of Staked SOL *은 (_ Inflation Schedule _ 매개 변수와 달리) 추정되어야하는 매개 변수이므로 특정 _ Inflation Schedule _ 매개 변수를 사용하고 * % of Staked SOL *의 범위를 탐색하는 것이 더 쉽습니다. 아래 예에서는 위에서 살펴본 매개 변수 범위의 중간을 선택했습니다.

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- -_ 초기 인플레이션 율 _ : 8 % -_ 디 인플레이션 율 _ : -15 % -_ 장기 인플레이션 율 _ : 1.5 %

% ~ \ text {SOL Staked} = \ frac {\ text {Total SOL Staked}} {\ text {Total Current Supply}}

![](/img/p_ex_staked_yields.png)

다시 말하지만, 위는 지정된대로 * Inflation Schedule *을 사용하여 Solana 네트워크에서 스테이커가 시간이 지남에 따라 예상 할 수있는 * Staked Yield *의 예를 보여줍니다. 이것은 보상, 밸리데이터 커미션, 잠재적 인 수익률 제한 및 잠재적 인 슬래싱 사고에 대한 밸리데이터 가동 시간 영향을 무시하므로 이상적인 * Staked Yield *입니다. 또한이 * 인플레이션 일정 *에 의해 설정된 경제적 인센티브 인 * Staked SOL *의 \* %가 설계 상 동적이라는 점을 추가로 무시합니다.

### 조정 된 스테이킹 수익률

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. . 즉 인플레이션의 긍정적 인 희석 효과.

인플레이션 율과 네트워크에 스테이킹 된 토큰 비율의 함수로 * 조정 된 스테이킹 수익률 *을 조사 할 수 있습니다. 여기에서 다양한 스테이킹 분수에 대해 플롯 된 것을 볼 수 있습니다.

![](/img/p_ex_staked_dilution.png)
