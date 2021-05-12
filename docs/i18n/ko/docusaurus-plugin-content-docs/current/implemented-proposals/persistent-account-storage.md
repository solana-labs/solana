---
title: Persistent Account Storage
---

## Persistent Account Storage

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. 각 밸리데이터은이 전체 세트를 유지해야합니다. Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

NVME와 같은 영구 스토리지는 DDR보다 20 ~ 40 배 저렴합니다. The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

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

## Garbage collection

As accounts get updated, they move to the end of the AppendVec. Once capacity has run out, a new AppendVec can be created and updates can be stored there. Eventually references to an older AppendVec will disappear because all the accounts have been updated, and the old AppendVec can be deleted.

To speed up this process, it's possible to move Accounts that have not been recently updated to the front of a new AppendVec. This form of garbage collection can be done without requiring exclusive locks to any of the data structures except for the index update.

The initial implementation for garbage collection is that once all the accounts in an AppendVec become stale versions, it gets reused. The accounts are not updated or moved around once appended.

## Index Recovery

Each bank thread has exclusive access to the accounts during append, since the accounts locks cannot be released until the data is committed. But there is no explicit order of writes between the separate AppendVec files. To create an ordering, the index maintains an atomic write version counter. Each append to the AppendVec records the index write version number for that append in the entry for the Account in the AppendVec.

To recover the index, all the AppendVec files can be read in any order, and the latest write version for every fork should be stored in the index.

## Snapshots

To snapshot, the underlying memory-mapped files in the AppendVec need to be flushed to disk. The index can be written out to disk as well.

## Performance

- -추가 전용 쓰기가 빠릅니다. 모든 OS 레벨 커널 데이터 구조뿐만 아니라 SSD 및 NVME는 PCI 또는 NVMe 대역폭이 \ (2,700MB / s \)를 허용하는만큼 빠르게 추가를 실행할 수 있도록합니다.
- Each replay and banking thread writes concurrently to its own AppendVec.
- Each AppendVec could potentially be hosted on a separate NVMe.
- Each replay and banking thread has concurrent read access to all the AppendVecs without blocking writes.
- Index requires an exclusive write lock for writes. Single-thread performance for HashMap updates is on the order of 10m per second.
- -Banking 및 Replay 스테이지는 NVMe 당 32 개의 스레드를 사용해야합니다. NVMe는 32 개의 동시 판독기 또는 기록기로 최적의 성능을 제공합니다.
