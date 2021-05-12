# 장기 RPC 거래 내역

RPC는 최소 6 개월의 거래 내역을 제공해야합니다. 현재 기록 (일 단위)은 다운 스트림 사용자에게 충분하지 않습니다.

6 개월 분량의 거래 데이터는 밸리데이터의 rocksdb 원장에 실질적으로 저장할 수 없으므로 외부 데이터 저장소가 필요합니다. 유효성 검사기의 rocksdb 원장은 계속해서 기본 데이터 소스로 사용 된 다음 외부 데이터 저장소로 대체됩니다.

영향을받는 RPC 엔드 포인트는 다음과 같습니다.

- [\[getFirstAvailableBlock\] (developing / clients / jsonrpc-api.md # getfirstavailableblock)](developing/clients/jsonrpc-api.md#getfirstavailableblock)
- [getConfirmedBlock](developing/clients/jsonrpc-api.md#getconfirmedblock)
- [getConfirmedBlocks](developing/clients/jsonrpc-api.md#getconfirmedblocks)
- [\[getConfirmedSignaturesForAddress\] (developing / clients / jsonrpc-api.md # getconfirmedsignaturesforaddress)](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress)
- [getConfirmedTransaction](developing/clients/jsonrpc-api.md#getconfirmedtransaction)
- [getSignatureStatuses](developing/clients/jsonrpc-api.md#getsignaturestatuses)

\[getConfirmedBlock\] (developing / clients / jsonrpc-api.md # getconfirmedblock)

일부 시스템 설계 제약 :

- 저장 및 검색 할 데이터의 양은 테라 바이트 단위로 빠르게 이동할 수 있으며 변경할 수 없습니다.
- 시스템은 SRE를 위해 가능한 한 가벼워 야합니다. 예를 들어 노드를 지속적으로 모니터링하고 재조정하기 위해 SRE가 필요한 SQL 데이터베이스 클러스터는 바람직하지 않습니다.
- 데이터는 실시간으로 검색 할 수 있어야합니다. 실행하는 데 몇 분 또는 몇 시간이 걸리는 일괄 쿼리는 허용되지 않습니다.
- 데이터를 전 세계에 쉽게 복제하여이를 활용할 RPC 엔드 포인트와 함께 배치 할 수 있습니다.
- 외부 데이터 저장소와의 인터페이스는 위험하고 가볍게 사용되는 커뮤니티 지원 코드 라이브러리에 따라 필요하지 않고 쉬워야합니다

이러한 제약 조건을 기반으로 Google의 BigTable 제품이 데이터 저장소로 선택됩니다.

## 테이블 스키마

BigTable 인스턴스는 모든 트랜잭션 데이터를 보관하는 데 사용되며 빠른 검색을 위해 여러 테이블로 나뉩니다.

새 데이터는 기존 데이터에 영향을주지 않고 언제든지 인스턴스에 복사 할 수 있으며 모든 데이터는 변경할 수 없습니다. 일반적으로 현재 시대가 완료되면 새 데이터가 업로드되지만 데이터 덤프 빈도에는 제한이 없습니다.

인스턴스 테이블의 데이터 보존 정책을 적절하게 구성하여 오래된 데이터를 자동으로 정리하면 사라집니다. 따라서 데이터가 추가되는 순서가 중요합니다. 예를 들어 N-1 시대의 데이터가 N 시대의 데이터 뒤에 추가되면 이전 시대의 데이터가 최신 데이터보다 오래갑니다. 그러나 쿼리 결과에서 *holes*를 생성하는 것 외에 이러한 종류의 정렬되지 않은 삭제는 아무런 영향을 미치지 않습니다. 이 정리 방법을 사용하면 거래 데이터를 무제한으로 저장할 수 있으며,이를 수행하는 데 드는 금전적 비용에 의해서만 제한됩니다.

테이블 레이아웃은 기존 RPC 끝점 만 지원합니다. 향후 새로운 RPC 끝점은 스키마에 추가하고 필요한 메타 데이터를 구축하기 위해 잠재적으로 모든 트랜잭션을 반복해야 할 수 있습니다.

## BigTable 액세스 BigTable

에는 \[tonic\] (https://crates.io/crates/crate)] 및 원시 protobuf API를 사용하여 액세스 할 수있는 gRPC 엔드 포인트가 있습니다. 현재 BigTable 용 상위 수준 Rust 상자가 없습니다. 실제로 이것은 BigTable 쿼리의 결과를 구문 분석하는 것을 더 복잡하게 만들지 만 중요한 문제는 아닙니다.

## 데이터 채우기채우기

주어진 슬롯 범위에 대한 rocksdb 데이터를 인스턴스 스키마로 변환하는 새로운`solana-ledger-tool` 명령을 사용하여 인스턴스 데이터의 지속적인가 한 시대에 발생합니다.

기존 원장 데이터를 채우기 위해 동일한 프로세스가 수동으로 한 번 실행됩니다.

### 블록 테이블 :`block`

이 테이블은 주어진 슬롯에 대한 압축 된 블록 데이터를 포함합니다.

행이 나열 될 때 확인 된 블록이있는 가장 오래된 슬롯이 항상 첫 번째가되도록하기 위해 행 키는 슬롯의 16 자리 소문자 16 진수 표현을 사용하여 생성됩니다. 예를 들어 슬롯 42의 행 키는 000000000000002a입니다.

행 데이터는 압축 된`StoredConfirmedBlock` 구조체입니다.

### 계정 주소 트랜잭션 서명 조회 테이블 :`tx-by-addr`

이 테이블에는 주어진 주소에 영향을주는 트랜잭션이 포함되어 있습니다.

행 키는`<base58 address> / <slot-id-one's-compliment-hex-slot-0-prefixed-to-16-digits>`입니다. 행 데이터는 압축 된`TransactionByAddrInfo` 구조체입니다.

슬롯의 칭찬을 받으면 슬롯 목록을 허용하면 주소에 영향을 미치는 트랜잭션이있는 최신 슬롯이 항상 먼저 나열됩니다.

Sysvar 주소는 인덱싱되지 않습니다. 그러나 투표 또는 시스템과 같이 자주 사용되는 프로그램은 확인 된 모든 슬롯에 대해 행이있을 가능성이 높습니다.

### 트랜잭션 서명 조회 테이블 :`tx`

이 테이블은 트랜잭션 서명을 확인 된 블록과 해당 블록 내의 인덱스에 매핑합니다.

행 키는 base58로 인코딩 된 트랜잭션 서명입니다. 행 데이터는 압축 된`TransactionInfo` 구조체입니다.
