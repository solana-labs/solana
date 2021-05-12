---
title: 클러스터 경제학
---

**\*\*** 변경 될 수 있습니다. Solana 포럼에서 가장 최근의 경제 토론을 따르십시오 : https : //forums.solana.com**\*\***

Solana의 암호화 경제 시스템은 네트워크의 보안 및 분산화에 맞춰 참가자 인센티브를 제공하여 건강하고 장기적인 자립 경제를 촉진하도록 설계되었습니다. 이 경제의 주요 참여자는 검증 클라이언트입니다. 네트워크에 대한 기여, 상태 검증 및 필수 인센티브 메커니즘은 아래에서 설명합니다.

참가자 송금의 주요 채널은 프로토콜 기반 (인플레이션) 보상 및 거래 수수료라고합니다. 프로토콜 기반 보상은 프로토콜에 정의 된 글로벌 인플레이션 비율에서 발행됩니다. 이러한 보상은 검증 클라이언트에게 전달되는 총 보상을 구성하며 나머지는 거래 수수료에서 발생합니다. 네트워크 초기에는 사전 정의 된 발행 일정에 따라 배포되는 프로토콜 기반 보상이 대부분의 참가자 인센티브가 네트워크에 참여하도록 유도 할 가능성이 있습니다.

이러한 프로토콜 기반 보상은 네트워크에서 적극적으로 스테이킹 된 토큰을 통해 분배되며, 글로벌 공급 인플레이션 율의 결과이며, Solana 시대에 따라 계산되고 활성 밸리데이터 세트에 분배됩니다. 아래에서 더 논의되는 바와 같이, 연간 인플레이션 율은 미리 결정된 인플레이션 일정을 기반으로합니다. 이를 통해 네트워크에 통화 공급 예측 가능성을 제공하여 장기적인 경제적 안정성과 보안을 지원합니다.

거래 수수료는 제안 된 거래의 포함 및 실행에 필요한 동기 및 보상으로 네트워크 상호 작용에 첨부 된 시장 기반 참가자 간 전송입니다. 장기적인 경제적 안정성을위한 메커니즘과 각 거래 수수료의 부분 소각을 통한 포크 보호에 대해서도 아래에서 설명합니다.

아래의 ** 그림 1 **에는 Solana의 암호화 경제 설계에 대한 개략적 인 개요가 나와 있습니다. 검증-클라이언트 경제학의 세부 사항은 \[Validation-client Economics\] (ed_validation_client_economics / ed_vce_overview.md), \[Inflation Schedule\] (ed_validation_client_economics / ed_vce_state_validation_protocol_validation_client_fees_validation_client_fees_validation_transaction Fees] (conomics.validation_transwards.vmd), \[Validation-client Economics\] (ed_validation_client_economics / ed_vce_overview.md) 섹션에 설명되어 있습니다. 또한 \[Validation Stake Delegation\] (ed_validation_client_economics / ed_vce_validation_stake_delegation.md) 섹션은 밸리데이터 위임 기회 및 시장에 대한 논의로 마무리됩니다. 또한 \[Storage Rent Economics\] (ed_storage_rent_economics.md)에서는 원장의 활성 상태를 유지하기위한 외부 비용을 설명하기 위해 스토리지 임대를 구현하는 방법을 설명합니다. MVP 경제 디자인의 기능 개요는 \[Economic Design MVP\] (ed_mvp.md) 섹션에서 설명합니다.

![](/img/economic_design_infl_230719.png)

** 그림 1 ** : Solana 경제적 인센티브 설계의 개략도.
