---
title: 유효성 검사기
---

## 역사

처음 Solana를 시작했을 때 목표는 TPS 클레임의 위험을 줄이는 것이 었습니다. 우리는 낙관적 인 동시성 제어와 충분히 긴 리더 슬롯 사이에서 PoS 합의가 TPS에 가장 큰 위험이 아니라는 것을 알고있었습니다. GPU 기반 서명 확인, 소프트웨어 파이프라이닝 및 동시 뱅킹이었습니다. 따라서 TPU가 탄생했습니다. 10 만 TPS를 달성 한 후, 우리는 팀을 71 만 TPS를 위해 일하는 하나의 그룹과 밸리데이터 파이프 라인을 구체화하기 위해 다른 그룹으로 나누었습니다. 따라서 TVU가 탄생했습니다. 현재 아키텍처는 해당 순서 및 프로젝트 우선 순위에 따른 점진적 개발의 결과입니다. 그것은 우리가 지금까지 믿었던 기술이 기술적으로 가장 우아한 단면이라고 믿었던 것을 반영한 것이 아닙니다. 리더 로테이션의 맥락에서 선두와 검증 사이의 강한 구분은 모호합니다.

## 유효성 검사와 선행의 차이점

파이프 라인 간의 근본적인 차이점은 역사증명가있을 때입니다. 리더에서는 트랜잭션을 처리하고 잘못된 트랜잭션을 제거한 다음 PoH 해시로 결과에 태그를 지정합니다. 유효성 검사기에서 해시를 확인하고 떼어 내고 정확히 동일한 방식으로 트랜잭션을 처리합니다. 유일한 차이점은 검증자가 잘못된 트랜잭션을 발견하면 리더처럼 단순히 제거 할 수 없다는 것입니다. 왜냐하면 PoH 해시가 변경 될 수 있기 때문입니다. 대신 전체 블록을 거부합니다. 파이프 라인 간의 다른 차이점은 _after_ banking입니다. 리더는 항목을 다운 스트림 유효성 검사기로 브로드 캐스트하는 반면 유효성 검사기는 확인 시간 최적화 인 RetransmitStage에서 이미이를 수행했을 것입니다. 반면에 유효성 검사 파이프 라인에는 마지막 단계가 있습니다. 블록 처리를 완료 할 때마다 관찰중인 포크의 무게를 측정하고 투표를 할 수 있으며, 그렇다면 역사증명 해시를 방금 투표 한 블록 해시로 재설정해야합니다.

## 제안 된 디자인

많은 추상화 계층을 풀고 밸리데이터의 ID가 리더 일정에 나타날 때마다 리더 모드를 전환 할 수있는 단일 파이프 라인을 구축합니다.

![Validator block diagram](/img/validator-proposal.svg)

## 주목할만한 변화

- Hoist FetchStage and BroadcastStage out of TPU
- -TPU에서 FetchStage 및 BroadcastStage 호이스트 -BankForks가 Banktree로 이름이 변경되었습니다.
- -TPU는 solana-tpu라는 새로운 소켓없는 상자로 이동합니다.
- -TPU의 BankingStage는 ReplayStage를 흡수합니다.
- TVU goes away
- -새로운 RepairStage는 Shred Fetch Stage 및 수리 요청을 흡수합니다.
- JSON RPC Service is optional - used for debugging. It should instead be part of a separate `solana-blockstreamer` executable.
- -새로운 MulticastStage는 RetransmitStage의 재전송 부분을 흡수합니다.
- -Blockstore의 MulticastStage 다운 스트림
