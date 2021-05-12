---
title: 결정 론적 거래 수수료
---

거래에는 현재 슬롯 리더가 거래를 처리하기 위해 청구 할 수있는 최대 수수료 필드를 나타내는 수수료 필드가 포함되어 있습니다. 반면에 클러스터는 최소 요금에 동의합니다. 네트워크가 정체되면 슬롯 리더는 더 높은 수수료를 제공하는 거래에 우선 순위를 둘 수 있습니다. 즉, 클러스터에서 트랜잭션을 확인하고 나머지 잔액을 확인할 때까지 클라이언트는 얼마나 많이 수집되었는지 알 수 없습니다. Ethereum의 "가스", 비결정론에 대해 우리가 싫어하는 냄새가납니다.

## 혼잡에 따른 수수료

각 유효성 검사기는 _ 슬롯 당 서명 _ \ (SPS \)을 사용하여 네트워크 정체를 추정하고 _SPS target_을 사용하여 클러스터의 원하는 처리 용량을 추정합니다. 유효성 검사기는 제네시스 구성에서 SPS 대상을 학습하는 반면 최근 처리 된 트랜잭션에서 SPS를 계산합니다. 제네시스 구성은 또한 클러스터가 _SPS target_에서 작동 할 때 서명 당 청구되는 요금 인 대상`lamports_per_signature`를 정의합니다.

## 수수료 계산

클라이언트는 JSON RPC API를 사용하여 클러스터에 현재 수수료 매개 변수를 쿼리합니다. 이러한 매개 변수는 블록 해시로 태그가 지정되고 해당 블록 해시가 슬롯 리더가 거부 할 수있을만큼 오래 될 때까지 유효합니다.

트랜잭션을 클러스터로 보내기 전에 클라이언트는 트랜잭션 및 수수료 계정 데이터를 _fee calculator_라는 SDK 모듈에 제출할 수 있습니다. 클라이언트의 SDK 버전이 슬롯 리더의 버전과 일치하는 한, 클라이언트는 수수료 계산기에서 반환 한 것과 정확히 동일한 수의 램프 포트가 변경 될 것입니다.

## 수수료 매개 변수

이 디자인의 첫 번째 구현에서 유일한 수수료 매개 변수는`lamports_per_signature`입니다. 클러스터에서 확인해야하는 서명이 많을수록 수수료가 높아집니다. 램프 포트의 정확한 수는 SPS 대 SPS 대상의 비율에 의해 결정됩니다. 각 슬롯의 끝에서 클러스터는 SPS가 대상 아래에있을 때`lamports_per_signature '를 낮추고 대상 위에있을 때 올립니다.`lamports_per_signature`의 최소값은 대상`lamports_per_signature``의 50 %이고 최대 값은 대상 \`lamports_per_signature '의 10 배입니다. The minimum value for``lamports_per_signature`is 50% of the target`lamports_per_signature` and the maximum value is 10x the target \`lamports_per_signature'

향후 매개 변수에는 다음이 포함될 수 있습니다.

- `lamports_per_pubkey`-계정로드 비용 -`lamports_per_slot_distance`-매우 오래된 계정을로드하는 데 더 높은 비용 -`lamports_per_byte`-로드 된 계정 크기 당 비용 -`lamports_per_bpf_instruction`-프로그램 실행 비용
- `lamports_per_slot_distance` - higher cost to load very old accounts
- `lamports_per_byte` - cost per size of account loaded
- `lamports_per_bpf_instruction` - cost to run a program

## 공격

### SPS 타겟 탈취

밸리데이터 그룹은 나머지 밸리데이터이 따라갈 수있는 지점 이상으로 SPS 타겟을 올리도록 설득 할 수있는 경우 클러스터를 중앙 집중화 할 수 있습니다. 목표를 높이면 수수료가 낮아져 더 많은 수요가 발생하여 TPS가 높아질 것입니다. 유효성 검사기에 그렇게 빠르게 많은 트랜잭션을 처리 할 수있는 하드웨어가없는 경우 확인 투표가 결국 너무 길어져 클러스터가 강제로 부팅해야합니다.
