---
title: Rust 클라이언트
---

## 문제

bench-tps와 같은 고수준 테스트는 '클라이언트'특성으로 작성됩니다. 테스트 스위트의 일부로 이러한 테스트를 실행할 때 낮은 수준의 'BankClient'구현을 사용합니다. 클러스터에 대해 동일한 테스트를 실행해야하는 경우 'ThinClient'구현을 사용합니다. 이 접근 방식의 문제점은 특성이 지속적으로 확장되어 새로운 유틸리티 기능을 포함하고 모든 구현에 새로운 기능을 추가해야한다는 것입니다. 네트워크 인터페이스를 추상화하는 트레이 트에서 사용자 대상 객체를 분리함으로써 트레이 트 및 그것의 구현.

## 제안 된 해법

`Client` 특성을 구현하는 대신`ThinClient`를 구현하여 구성해야합니다. 이렇게하면 현재`Client` 트레이 트에있는 모든 유틸리티 함수가`ThinClient`로 이동할 수 있습니다. 그러면`ThinClient`는 모든 네트워크 종속성이`Client` 구현에 있기 때문에`solana-sdk`로 이동할 수 있습니다. 그런 다음 'ClusterClient'라고하는 '클라이언트'의 새로운 구현을 추가하면 현재 'ThinClient'가있는 'solana-client'크레이트에 있습니다.

이 재구성 후 클라이언트가 필요한 모든 코드는`ThinClient`로 작성됩니다. 단위 테스트에서 기능은`ThinClient <BankClient>`로 호출되는 반면`main ()`함수, 벤치 마크 및 통합 테스트는`ThinClient <ClusterClient>`로 호출됩니다.

더 높은 수준의 구성 요소가 'BankClient'로 구현할 수있는 것보다 더 많은 기능을 필요로하는 경우 여기에 설명 된 것과 동일한 패턴에 따라 두 번째 특성을 구현하는 두 번째 개체에 의해 구현되어야합니다.

### 오류 처리

`Client`는`Custom (String)`필드를`Custom (Box <dyn Error>)`로 변경해야한다는 점을 제외하고는 기존`TransportError` 열거 형을 오류에 사용해야합니다.

### 구현 전략

1. Add new object to `solana-sdk`, `ThinClientTng`; initialize it with `RpcClientTng` and an `AsyncClient` implementation
2. `RpcClientTng` 및`AsyncClient` 구현으로 초기화합니다.
3. 1.`solana-sdk`,`RpcClientTng`에 새 개체를 추가합니다.
4. 모든 단위 테스트를 'BankClient'에서 'ThinClientTng
5. Add `ClusterClient`
6. '로 이동합니다. 5.`ClusterClient` 추가 6.`ThinClient` 사용자를`ThinClientTng <ClusterClient>`로 이동합니다.
7. 7.`ThinClient`를 삭제하고`ThinClientTng`의 이름을`ThinClient`로 바꿉니다.
8. 8.`RpcClient` 사용자를 새`ThinClient <ClusterClient>`로 이동합니다.
9. 9.`RpcClient`를 삭제하고`RpcClientTng`의 이름을`RpcClient`로 바꿉니다.
