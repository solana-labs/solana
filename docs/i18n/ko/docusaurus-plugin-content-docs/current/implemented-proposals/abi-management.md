---
title: Solana ABI 관리 프로세스
---

이 문서는 Solana ABI 관리 프로세스를 제안합니다. ABI 관리 프로세스는 의도하지 않은 호환되지 않는 ABI 변경을 방지하기위한 엔지니어링 관행이자 지원 기술 프레임 워크입니다.

# 문제

Solana ABI (클러스터에 대한 바이너리 인터페이스)는 현재 구현에 의해 묵시적으로 만 정의되며 주요 변경 사항을 확인하기 위해 매우 신중한 눈이 필요합니다. 이로 인해 원장을 재부팅하지 않고 기존 클러스터에서 소프트웨어를 업그레이드하기가 매우 어렵습니다.

# 요구 사항 및 목표

- -의도하지 않은 ABI 변경은 CI 오류로 기계적으로 감지 될 수 있습니다.
- -새로운 구현은 일단 우리가 메인 넷으로 가면 가장 오래된 데이터 (제네시스 이후)를 처리 할 수 ​​있어야합니다.
- -이 제안의 목적은 매우 긴 인간 중심의 감사 프로세스가 아닌 기계적 프로세스를 선택하여 다소 빠른 개발을 유지하면서 ABI를 보호하는 것입니다.
- -암호화 방식으로 서명되면 데이터 Blob이 동일해야하므로 온라인 시스템의 인바운드 및 아웃 바운드에 관계없이 인플레 이스 데이터 형식 업데이트가 불가능합니다. 또한 처리하려는 트랜잭션의 양을 고려할 때 소급 내부 업데이트는 기껏해야 바람직하지 않습니다.

# Solution

정기적으로 실패한다고 가정해야하는 자연스러운 인간의 눈 실사 대신 소스 코드 변경시 클러스터를 깨지 않는 체계적인 보증이 필요합니다.

이를 위해 소스 코드 (`struct`s,`enum`s)의 모든 ABI 관련 항목을 새로운`# [frozen_abi]`속성으로 표시하는 메커니즘을 도입합니다. 이것은`ser :: Serialize`를 통해 필드 유형에서 파생 된 하드 코딩 된 다이제스트 값을 사용합니다. 그리고 속성은 표시된 ABI 관련 항목에 대한 승인되지 않은 변경 사항을 감지하기 위해 자동으로 단위 테스트를 생성합니다.

그러나 감지가 완료 될 수 없습니다. 소스 코드를 정적으로 분석하는 것이 아무리 어렵더라도 ABI를 깨는 것은 여전히 ​​가능합니다. 예를 들어, 여기에는 '파생되지 않은'손으로 쓴 'ser :: Serialize', 기본 라이브러리의 구현 변경 (예 : 'bincode'), CPU 아키텍처 차이가 포함됩니다. 이러한 가능한 ABI 비 호환성 감지는이 ABI 관리의 범위를 벗어납니다.

# 정의

ABI 항목 / 유형 : 직렬화에 사용되는 다양한 유형으로, 모든 시스템 구성 요소에 대한 전체 ABI를 집합 적으로 구성합니다. 예를 들어, 이러한 유형에는`struct` 및`enum`이 포함됩니다.

ABI item digest: Some fixed hash derived from type information of ABI item's fields.

# 예제

```patch
+ # [frozen_abi (digest = "eXSMM7b89VY72V ...")]
 # [derive (Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
 pub struct Vote {
     /// 스택 가장 오래된 투표부터 시작하는 투표
     pub 슬롯: Vec <Slot>,
     /// 마지막 슬롯에서 은행의 상태 서명
     pub 해시: Hash,
 }
```

# 개발자 워크 플로

ABI 항목 다이제스트 : ABI 항목 필드의 유형 정보에서 파생 된 일부 고정 해시입니다.

일반적으로`frozen_abi`를 추가하고 해당 변경 사항이 stable 릴리스 채널에 게시되면 다이제스트가 변경되지 않아야합니다. 그러한 변경이 필요하다면, 우리는`FooV1`과 같은 새로운`struct`를 정의하는 것을 선택해야합니다. 그리고 하드 포크와 같은 특수 릴리스 흐름에 접근해야합니다.

# 구현 설명

우리는 단위 테스트를 자동으로 생성하고 ABI 항목에서 다이제스트를 계산하기 위해 어느 정도의 매크로 기계를 사용합니다. 이는`serde :: Serialize` (`[1]`) 및`any :: type_name` (`[2]`)을 현명하게 사용하여 수행 할 수 있습니다. 유사한 구현의 선례로는 Parity Technologies`[3]`의`ink`가 정보 용일 수 있습니다.

# 구현 세부 정보

구현의 목표는 의도하지 않은 ABI 변경을 최대한 자동으로 감지하는 것입니다. 이를 위해 구조적 ABI 정보의 다이제스트가 최선의 정확도와 안정성으로 계산됩니다.

ABI 다이제스트 검사가 실행되면 'serde'의 직렬화 기능, proc 매크로 및 일반 전문화를 재사용하여 ABI 항목 필드의 ABI를 재귀 적으로 다이제스트하여 ABI 다이제스트를 동적으로 계산합니다. 그리고 나서 최종 다이제스트 값이`frozen_abi` 속성에 지정된 것과 동일한 지`assert!`를 확인합니다.

이를 실현하기 위해 유형의 예제 인스턴스와`serde`에 대한 사용자 정의`Serializer` 인스턴스를 생성하여 마치 실제 예제를 직렬화하는 것처럼 필드를 재귀 적으로 탐색합니다. This traversing must be done via `serde` to really capture what kinds of data actually would be serialized by `serde`, even considering custom non-`derive`d `Serialize` trait implementations.

# ABI 다이제스트 프로세스이

부분은 약간 복잡합니다. 'AbiExample', 'AbiDigester'및 'AbiEnumVisitor'의 세 가지 상호 종속적 인 부분이 있습니다.

먼저 생성 된 테스트는`AbiExample`이라는 트레이 트가있는 다이제스트 된 유형의 예제 인스턴스를 생성합니다.이 인스턴스는`Serialize`와 같은 모든 다이제스트 유형에 대해 구현되어야하며`Default` 트레이 트와 같이`Self`를 반환해야합니다. 일반적으로 대부분의 일반적인 유형에 대한 일반 특성 전문화를 통해 제공됩니다. 또한`struct`와`enum`에 대해`derive`가 가능하며 필요한 경우 손으로 작성할 수 있습니다.

커스텀`Serializer`는`AbiDigester`라고합니다. 그리고 일부 데이터를 직렬화하기 위해`serde`가 호출하면 가능한 한 많은 ABI 정보를 재귀 적으로 수집합니다. ABI 다이제스트에 대한`AbiDigester`의 내부 상태는 데이터 유형에 따라 다르게 업데이트됩니다. 이 로직은 각`enum` 유형에 대해`AbiEnumVisitor`라는 특성을 통해 특별히 리디렉션됩니다. 이름에서 알 수 있듯이 다른 유형에 대해서는`AbiEnumVisitor`를 구현할 필요가 없습니다.

이 상호 작용을 요약하면`serde`는`AbiDigester`와 함께 재귀 직렬화 제어 흐름을 처리합니다. 테스트 및 하위`AbiDigester`의 초기 진입 점은`AbiExample`을 재귀 적으로 사용하여 예제 객체 계층 그래프를 만듭니다. 그리고`AbiDigester`는`AbiEnumVisitor`를 사용하여 구성된 샘플을 사용하여 실제 ABI 정보를 조회합니다.

'기본값'은 'AbiExample'에 충분하지 않습니다. 다양한 컬렉션의`:: default ()`는 비어 있지만 실제 항목으로 요약하고 싶습니다. 그리고 ABI 다이제스트는 'AbiEnumVisitor'만으로는 실현 될 수 없습니다. 실제로 'serde'를 통해 데이터를 순회하려면 실제 유형의 인스턴스가 필요하기 때문에 'AbiExample'이 필요합니다.

반면 ABI 다이제스트는 'AbiExample'로만 수행 할 수 없습니다. `AbiEnumVisitor`는 '열거 형'의 모든 변형이 ABI 예제로 단일 변형으로 만 순회 될 수 없기 때문에 필요합니다.

Digestable information:

- rust's type name
- `serde`'s data type name
- all fields in `struct`
- all variants in `enum`
- `struct`: normal(`struct {...}`) and tuple-style (`struct(...)`)
- `enum`: normal variants and `struct`- and `tuple`- styles.
- -속성 :`serde (serialize_with = ...)`및`serde (skip)`

소화 가능한 정보 :-rust

- 제공하는 샘플에서 건드리지 않은 모든 사용자 정의 직렬화 코드 경로. (technically not possible)
- 구체적인 유형 별칭에는`frozen_abi`를 사용)

# 참조

1. [\[(De) Serialization with type info · Issue # 1095 · serde-rs / serde\] (https : / /github.com/serde-rs/serde/issues/1095#issuecomment-345483479)](https://github.com/serde-rs/serde/issues/1095#issuecomment-345483479)
2. [[`std :: any :: type_name`-Rust] (https://doc.rust-lang.org/std/any /fn.type_name.html)](https://doc.rust-lang.org/std/any/fn.type_name.html)
3. [\[스마트 컨트랙트을 작성하는 패리티의 잉크\] (https://github.com/paritytech/ink)](https://github.com/paritytech/ink)
