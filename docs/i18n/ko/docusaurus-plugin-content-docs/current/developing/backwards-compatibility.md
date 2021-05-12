---
title: 하위 호환성 정책
---

Solana 개발자 에코 시스템이 성장함에 따라 Solana 용으로 구축 된 애플리케이션 및 도구에 영향을 미치는 API 및 동작 변경에 대한 명확한 기대치가 필요합니다. 완벽한 세상에서 Solana 개발은 기존 개발자에게 문제를 일으키지 않고 매우 빠른 속도로 계속 될 수 있습니다. 그러나 일부 타협이 필요하므로이 문서는 새 릴리스에 대한 프로세스를 명확히하고 코드화하려고합니다.

### 예상

- 'MINOR'버전 릴리스 용 소프트웨어는 동일한 'MAJOR'버전의 모든 소프트웨어에서 호환됩니다.
- Web3.JS SDK도 의미 론적 버전 지정 사양을 따르지만 Solana 소프트웨어 릴리스와 별도로 제공됩니다.
- 향후`MAJOR` 릴리스에서는 더 이상 사용되지 않는 기능이 호환되지 않는 방식으로 제거됩니다.

### 폐기 프로세스

1. 코드 업그레이드 난이도에 따라 일부 기능은 몇 가지 릴리스주기 동안 더 이상 사용되지 않습니다.
2. According to code upgrade difficulty, some features will be remain deprecated for a few release cycles.
3. In a future `MAJOR` release, deprecated features will be removed in an incompatible way.

### 릴리스 케이던스

Solana RPC API, Rust SDK, CLI 도구 및 BPF 프로그램 SDK는 모두 각 Solana 소프트웨어 릴리스와 함께 업데이트 및 제공되며 특정`MINOR` 버전 릴리스의`PATCH` 업데이트간에 항상 호환되어야합니다.

#### 릴리스 채널

- [`solana-sdk`] ( https://docs.rs/solana-sdk/)-트랜잭션 생성 및 계정 상태 구문 분석을위한 Rust SDK
- [`solana-program`] (https://docs.rs/solana-program/)-작성을위한 Rust SDK 프로그램
- [`solana-client`] (https://docs.rs/solana-client/)-RPC API에 연결하기위한 Rust 클라이언트

#### 부 릴리스 (1.x.0)

`MAJOR '버전 릴리스 (예 : 2.0.0)에는 이전에 사용되지 않는 기능의 주요 변경 사항 및 제거가 포함될 수 있습니다. 클라이언트 SDK 및 도구는 이전`MAJOR` 버전에서 활성화 된 새로운 기능과 엔드 포인트를 사용하기 시작합니다.

#### 패치 릴리스 (1.0.x)

새로운 기능 및 제안 구현이 _new_`MINOR '버전 릴리스 (예 : 1.4.0)에 추가되었으며 Solana의 Tour de SOL 테스트 넷 클러스터에서 처음 실행됩니다. 테스트 넷에서 실행되는 동안 'MINOR'버전은 '베타'출시 채널에있는 것으로 간주됩니다. 이러한 변경 사항이 필요에 따라 패치되고 신뢰할 수있는 것으로 입증 된 후`MINOR`버전은`stable` 릴리스 채널로 업그레이드되고 메인 넷 베타 클러스터에 배포됩니다.

#### 공용 API 노드

낮은 위험 기능, 주요 변경 사항, 보안 및 버그 수정은 'PATCH'버전 릴리스 (예 : 1.0.11)의 일부로 제공됩니다. 패치는 '베타'및 '안정된'출시 채널 모두에 적용될 수 있습니다.

### RPC API

Patch releases:
- Bug fixes
- Security fixes
- Endpoint / feature deprecation

Minor releases:
- New RPC endpoints and features

Major releases:
- Removal of deprecated features

### Rust Crates

* [`solana-sdk`](https://docs.rs/solana-sdk/) - Rust SDK for creating transactions and parsing account state
* [`solana-cli-config`] (https://docs.rs/ solana-cli-config /)-Solana CLI 구성 파일 관리를위한 Rust 클라이언트
* [`solana-client`](https://docs.rs/solana-client/) - Rust client for connecting to RPC API
* [`solana-cli-config`](https://docs.rs/solana-cli-config/) - Rust client for managing Solana CLI config files

Patch releases:
- Bug fixes
- Security fixes
- Performance improvements

Minor releases:
- New APIs

부 릴리스 : -새로운 RPC 엔드 포인트 및 기능
- 주요 릴리스 -더 이상 사용되지 않는 API 제거 -이전 버전과 호환되지 않는 동작 변경
- Backwards incompatible behavior changes

### CLI 도구

Patch releases:
- Bug and security fixes
- Performance improvements
- Subcommand / argument deprecation

Minor releases:
- 부 릴리스 : -새 하위 명령

Major releases:
- 주요 릴리스 : -새 RPC API 끝점 / 구성으로 전환 이전 주 버전에서 소개되었습니다.
- Removal of deprecated features

### 런타임 기능

새로운 Solana 런타임 기능은 기능이 전환되고 수동으로 활성화됩니다. 런타임 기능은 다음과 같습니다 : 새로운 네이티브 프로그램, sysvars 및 syscalls 도입; 행동의 변화. 기능 활성화는 클러스터에 구애받지 않으므로 Mainnet-beta에서 활성화하기 전에 Testnet에서 신뢰를 구축 할 수 있습니다.

패치 릴리스 : -버그 및 보안 수정 -성능 개선 -하위 명령 / 인수 사용 중단

1. 새 런타임 기능이 새 릴리스에 포함되어 기본적으로 비활성화됩니다 .
2. 충분한 스테이킹 밸리데이터가 새 릴리스로 업그레이드되면 런타임 기능 스위치가 지침과 함께 수동으로 활성화됩니다.
3. 기능이 적용됩니다. 다음 시대가 시작될 때

### 인프라 변경 사항

#### 로컬 클러스터 스크립트 및 Docker 이미지

Solana는 모든 개발자가 사용할 수있는 공개적으로 사용 가능한 RPC API 노드를 제공합니다. Solana 팀은 호스트, 포트, 속도 제한 동작, 가용성 등에 대한 변경 사항을 전달하기 위해 최선을 다할 것입니다. 그러나 개발자는 자체 밸리데이터 노드에 의존하여 Solana 운영 노드에 대한 의존성을 억제 할 것을 권장합니다.

#### Local cluster scripts and Docker images

주요 변경 사항은 'MAJOR'버전 업데이트로 제한됩니다. 'MINOR'및 'PATCH'업데이트는 항상 이전 버전과 호환되어야합니다.

### 예외

#### Web3 JavaScript SDK

The Web3.JS SDK also follows semantic versioning specifications but is shipped separately from Solana software releases.

#### 공격 벡터

릴리스 프로세스는 다음과 같습니다.
