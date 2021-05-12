---
title: 헌신
---

약정 지표는 고객에게 특정 블록에 대한 네트워크 확인 및 지분 수준의 측정치를 제공하는 것을 목표로합니다. 그런 다음 고객은이 정보를 사용하여 자신의 약속 척도를 도출 할 수 있습니다.

# 계산 RPC

클라이언트는 RPC를 통해`get_block_commitment (s : Signature)-> BlockCommitment`를 통해 서명`s`에 대한 밸리데이터에게 약정 메트릭을 요청할 수 있습니다. `BlockCommitment` 구조체는 u64`[u64, MAX_CONFIRMATIONS]`의 배열을 포함합니다. 이 배열은 유효성 검사기가 투표 한 마지막 블록 'M'의 서명 's'를 포함하는 특정 블록 'N'에 대한 약정 지표를 나타냅니다.

`BlockCommitment` 배열의 인덱스`i`에있는 항목`s`는 밸리데이터가 일부 블록`M`에서 관찰 된대로 블록`N`에서`i` 확인에 도달하는 클러스터의`s` 총 지분을 관찰했음을 의미합니다. 이 배열에는 'MAX_CONFIRMATIONS'요소가 있으며 1부터 'MAX_CONFIRMATIONS'까지 가능한 모든 확인 수를 나타냅니다.

# 약정 메트릭 계산

이`BlockCommitment` 구조체를 구축하면 합의 구축을 위해 이미 수행중인 계산을 활용합니다. 'consensus.rs'의 'collect_vote_lockouts'함수는 각 항목이 '(b, s)'형식이며, 여기서 's'는 은행 'b'의 지분 금액입니다.

이 계산은 다음과 같이 투표 가능한 후보 뱅크 'b'에서 수행됩니다.

```text
   출력하자 : HashMap <b, Stake> = HashMap :: new ();
   for vote_account in b.vote_accounts {
       v in vote_account.vote_stack {
           for a in ancestors (v) {
               f (* output.get_mut (a), vote_account, v);
           }
       }
   }
```

여기서 'f'는 투표 'v'및 'vote_account'(스테이킹, 잠금 등)에서 파생 된 일부 데이터로 슬롯 'a'의 '스테이킹'항목을 수정하는 일부 누적 함수입니다. 여기서 '상위'에는 현재 상태 캐시에있는 슬롯 만 포함된다는 점에 유의하세요. ㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ 상태 캐시에있는 것보다 이전의 뱅크에 대한 서명은 어쨌든 쿼리 할 수 ​​없으므로 이러한 뱅크는 여기의 약정 계산에 포함되지 않습니다.

이제 자연스럽게 위의 계산을 보강하여 모든 뱅크 'b'에 대한 'BlockCommitment'배열을 다음과 같이 구축 할 수 있습니다.

1. 위의 계산이 모든 뱅크 'b'에 대해이 'BlockCommitment'를 구축하도록 'f'를 'f'로 대체합니다.
2. Replacing `f` with `f'` such that the above computation also builds this `BlockCommitment` for every bank `b`.

1.`BlockCommitment` 구조체를 수집하기 위해`ForkCommitmentCache` 추가

1)은 사소하기 때문에 2)의 세부 사항을 진행할 것입니다.

Now more specifically, we augment the above computation to:

```text
   출력하자 : HashMap <b, Stake> = HashMap :: new ();
   let fork_commitment_cache = ForkCommitmentCache :: default ();
   for vote_account in b.vote_accounts {
       // 투표 스택은 가장 오래된 투표에서 최신 투표로 정렬됩니다. for (v1, v2) in vote_account.vote_stack.windows (2) {
           for a in ancestors (v1) .difference (ancestors (v2)) {
               f '(* output.get_mut (a), * fork_commitment_cache.get_mut (a), vote_account, v);
           }
       }
   }
```

이제 더 구체적으로 위의 계산을 다음과 같이 확장합니다.

```text
    fn f` (
        스테이킹 : & mut 스테이킹,
        some_ancestor : & mut BlockCommitment,
        vote_account : VoteAccount,
        v : 투표, total_stake : u64
    ) {
        f (stake, vote_account, v);
        * some_ancestor.commitment [v.num_confirmations] + = vote_account.stake;
    }
```
