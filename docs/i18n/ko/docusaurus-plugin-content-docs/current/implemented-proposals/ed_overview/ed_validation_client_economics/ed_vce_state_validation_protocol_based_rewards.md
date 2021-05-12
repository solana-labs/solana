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

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

첫 번째 요소는 프로토콜 매개 변수의 기능으로 \ (즉, 주어진 시대의 밸리데이터 행동과는 무관 함 \) 조기 참여를 장려하고 명확한 통화 안정성을 제공하며 네트워크에서 최적의 보안을 제공하도록 설계된 인플레이션 일정을 생성합니다.

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

구체적으로 특별히:

![](/img/p_inflation_schedule_ranges_w_comments.png)

위의 그래프에서 범위의 평균 값은 각 매개 변수의 기여도를 보여주기 위해 식별됩니다. From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. 아래 예에서는 위에서 살펴본 매개 변수 범위의 중간을 선택했습니다.

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### 조정 된 스테이킹 수익률

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. . 즉 인플레이션의 긍정적 인 희석 효과.

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. 여기에서 다양한 스테이킹 분수에 대해 플롯 된 것을 볼 수 있습니다.

![](/img/p_ex_staked_dilution.png)
