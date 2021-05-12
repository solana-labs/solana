---
title: Persistent Account Storage
---

## Persistent Account Storage

계정 세트는 밸리데이터이 처리 한 모든 트랜잭션의 현재 계산 된 상태를 나타냅니다. 각 밸리데이터은이 전체 세트를 유지해야합니다. 네트워크에서 제안한 각 블록은이 세트에 대한 변경 사항을 나타내며 각 블록은 잠재적 인 롤백 지점이므로 변경 사항을 되돌릴 수 있어야합니다.

NVME와 같은 영구 스토리지는 DDR보다 20 ~ 40 배 저렴합니다. 영구 저장소의 문제는 쓰기 및 읽기 성능이 DDR보다 훨씬 느리고 데이터를 읽거나 쓰는 방법에주의를 기울여야한다는 것입니다. 읽기와 쓰기는 모두 여러 스토리지 드라이브로 분할되어 병렬로 액세스 할 수 있습니다. 이 설계는 스토리지의 동시 읽기 및 동시 쓰기를 허용하는 데이터 구조를 제안합니다. 쓰기는 AppendVec 데이터 구조를 사용하여 최적화됩니다.이 구조를 사용하면 단일 작성자가 추가하는 동시에 여러 판독기에 액세스 할 수 있습니다. 계정 인덱스는 계정이 모든 포크에 추가 된 지점에 대한 포인터를 유지하므로 명시적인 상태 체크 포인트가 필요하지 않습니다.

## AppendVec

AppendVec은 단일 추가 전용 작성기와 동시에 임의 읽기를 허용하는 데이터 구조입니다. AppendVec의 용량을 늘리거나 크기를 조정하려면 독점 액세스가 필요합니다. 이것은 완료된 추가가 끝날 때 업데이트되는 원자 적 '오프셋'으로 구현됩니다.

AppendVec의 기본 메모리는 메모리 매핑 파일입니다. 메모리 매핑 파일은 빠른 임의 액세스를 허용하며 페이징은 OS에서 처리합니다.

## 계정 색인

계정 색인은 현재 분기 된 모든 계정에 대해 단일 색인을 지원하도록 설계되었습니다.

```text
유형 AppendVecId = 사용;

유형 Fork = u64;

struct AccountMap (Hashmap <Fork, (AppendVecId, u64)>);

유형 AccountIndex = HashMap <Pubkey, AccountMap>;
```

인덱스는 AppendVec의 계정 데이터 위치와 Forks 맵에 대한 계정 Pubkeys의 맵입니다. 특정 Fork에 대한 계정 버전을 얻으려면 :

```text
/// pubkey에 대한 계정을로드합니다.
///이 함수는 지정된 포크에서 계정을로드하고, 포크의 부모로
폴백합니다. /// * fork-Fork로 키가 지정된 가상 계정 인스턴스입니다.  계정은 Forks,를 사용하여 부모를 추적
/// 영구 저장소
/// * pubkey-계정의 공개 키합니다.
pub fn load_slow (& self, id : Fork, pubkey : & Pubkey)-> Option <& Account>
```

저장된 오프셋의`AppendVecId`에서 메모리 매핑 된 위치를 가리키면 읽기가 충족됩니다. 참조는 복사본없이 반환 될 수 있습니다.

### 루트 포크

\[Tower BFT\] (tower-bft.md)는 결국 포크를 루트 포크로 선택하고 포크가 스쿼시됩니다. 스쿼시 / 루트 포크는 롤백 할 수 없습니다.

포크가 스쿼시되면, 아직 포크에없는 상위 계정의 모든 계정은 인덱스를 업데이트하여 포크로 가져옵니다. 스쿼시 된 포크의 잔액이 0 인 계정은 인덱스를 업데이트하여 포크에서 제거됩니다.

스 쿼싱으로 인해 도달 할 수없는 경우 계정은 * 가비지 수집 *이 될 수 있습니다.

세 가지 가능한 옵션이 있습니다

- .-루트 포크의 HashSet을 유지합니다. 매초마다 하나가 생성 될 것으로 예상됩니다. 나중에 전체 트리를 가비지 수집 할 수 있습니다. 또는 모든 포크가 계정의 참조 수를 유지하는 경우 인덱스 위치가 업데이트 될 때마다 가비지 수집이 발생할 수 있습니다.
- Remove any pruned forks from the index. 루트보다 숫자가 낮은 나머지 포크는 루트로 간주 될 수 있습니다.
- Scan the index, migrate any old roots into the new one. 새 루트보다 낮은 나머지 포크는 나중에 삭제할 수 있습니다.

## 추가 전용 쓰기

계정에 대한 모든 업데이트는 추가 전용 업데이트로 발생합니다. 모든 계정 업데이트에 대해 새 버전이 AppendVec에 저장됩니다.

포크에 이미 저장된 계정에 대한 변경 가능한 참조를 반환하여 단일 포크 내에서 업데이트를 최적화 할 수 있습니다. 은행은 이미 계정의 동시 액세스를 추적하고 특정 계정 포크에 대한 쓰기가 해당 포크의 계정에 대한 읽기와 동시에 발생하지 않도록 보장합니다. 이 작업을 지원하기 위해, AppendVec이 기능을 구현해야합니다

```text
:<code>텍스트
FN의 get_mut (자기 인덱스 : U64) -> & MUT T;</code>
```

이 API는`index`의 메모리 영역에 대한 동시 변경 가능한 액세스를 허용합니다. 해당 지수에 대한 독점적 접근을 보장하기 위해 은행에 의존합니다.

## 가비지 수집

계정이 업데이트되면 AppendVec의 끝으로 이동합니다. 용량이 부족하면 새 AppendVec을 생성하고 업데이트를 저장할 수 있습니다. 모든 계정이 업데이트되고 이전 AppendVec을 삭제할 수 있기 때문에 결국 이전 AppendVec에 대한 참조가 사라집니다.

이 프로세스의 속도를 높이기 위해 최근에 업데이트되지 않은 계정을 새 AppendVec의 맨 앞으로 이동할 수 있습니다. 이러한 형태의 가비지 수집은 인덱스 업데이트를 제외한 모든 데이터 구조에 대한 배타적 잠금없이 수행 할 수 있습니다.

가비지 콜렉션의 초기 구현은 AppendVec의 모든 계정이 오래된 버전이되면 재사용되는 것입니다. 계정이 추가되면 업데이트되거나 이동되지 않습니다.

## 인덱스 복구

데이터가 커밋 될 때까지 계정 잠금을 해제 할 수 없기 때문에 각 뱅크 스레드는 추가 중에 계정에 독점적으로 액세스 할 수 있습니다. 그러나 별도의 AppendVec 파일 간에는 명시적인 쓰기 순서가 없습니다. 순서를 생성하기 위해 인덱스는 원자 쓰기 버전 카운터를 유지합니다. AppendVec에 대한 각 추가는 AppendVec의 계정 항목에 해당 추가에 대한 색인 쓰기 버전 번호를 기록합니다.

인덱스를 복구하려면 모든 AppendVec 파일을 순서에 관계없이 읽을 수 있으며 모든 포크에 대한 최신 쓰기 버전을 인덱스에 저장해야합니다.

## 스냅 샷 스냅 샷

을 찍으려면 AppendVec의 기본 메모리 매핑 파일을 디스크로 플러시해야합니다. 인덱스는 디스크에도 쓸 수 있습니다.

## 성능

- -추가 전용 쓰기가 빠릅니다. 모든 OS 레벨 커널 데이터 구조뿐만 아니라 SSD 및 NVME는 PCI 또는 NVMe 대역폭이 \ (2,700MB / s \)를 허용하는만큼 빠르게 추가를 실행할 수 있도록합니다.
- Each replay and banking thread writes concurrently to its own AppendVec.
- Each AppendVec could potentially be hosted on a separate NVMe.
- Each replay and banking thread has concurrent read access to all the AppendVecs without blocking writes.
- Index requires an exclusive write lock for writes. Single-thread performance for HashMap updates is on the order of 10m per second.
- -Banking 및 Replay 스테이지는 NVMe 당 32 개의 스레드를 사용해야합니다. NVMe는 32 개의 동시 판독기 또는 기록기로 최적의 성능을 제공합니다.
