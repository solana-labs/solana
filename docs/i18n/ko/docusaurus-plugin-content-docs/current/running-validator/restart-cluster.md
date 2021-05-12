## 클러스터 다시 시작

### 1 단계. 클러스터가 다시 시작될 슬롯 식별

낙관적으로 확인 된 가장 높은 슬롯은 시작하기에 가장 좋은 슬롯이며 \[this\] (https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#)를 검색하여 찾을 수 있습니다. L71) 메트릭 데이터 포인트. 그렇지 않으면 마지막 루트를 사용하십시오.

이 슬롯을`SLOT_X`라고합니다.

### 2 단계. 유효성 검사기 중지

### 3 단계. 선택적으로 새 솔라나 버전 설치

### 4 단계. 슬롯`SLOT_X`에서 하드 포크를 사용하여 슬롯`SLOT_X`에 대한 새 스냅 샷을 만듭니다.

```bash
$ solana-ledger-tool -l 원장 생성-스냅 샷 SLOT_X 원장 --hard-fork SLOT_X
```

이제 원장 디렉토리에 새 스냅 샷이 포함되어야합니다. `solana-ledger-tool create-snapshot` will also output the new shred version, and bank hash value, call this NEW_SHRED_VERSION and NEW_BANK_HASH respectively.

밸리데이터의 인수를 조정하십시오.

```bash
 -초대 다수 대기 SLOT_X
 -예상 은행 해시 NEW_BANK_HASH
```

그런 다음 유효성 검사기를 다시 시작하십시오.

로그를 통해 유효성 검사기가 부팅되었고 현재 'SLOT_X'에서 보류 패턴에 있으며 과반수를 기다립니다.

### 5 단계. Discord에서 다시 시작 알림 :

announcements에 다음과 같이 게시합니다 (텍스트를 적절하게 조정).

> 안녕하세요 @Validators 님,
>
> v1.1.12를 출시했으며 테스트 넷을 다시 백업 할 준비가되었습니다.
>
> Steps:
>
> 1. Install the v1.1.12 release: https://github.com/solana-labs/solana/releases/tag/v1.1.12
> 2. a. 선호하는 방법은 다음과 같이 로컬 원장에서 시작합니다.
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --no-untrusted-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```

````

b. 유효성 검사기에 SLOT_X 슬롯까지 원장이 없거나 원장을 삭제 한 경우 대신 다음을 사용하여 스냅 샷을 다운로드하도록합니다.

```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
````

     원장에있는 슬롯을 확인할 수 있습니다 :`solana-ledger-tool -l path / to / ledger bounds`

3. 지분의 80 %가 온라인 상태가 될 때까지 기다리십시오.

다시 시작된 유효성 검사기가 80 %를 올바르게 기다리고 있는지 확인하려면 : a. Look for `N% of active stake visible in gossip` log messages b. RPC를 통해 어떤 슬롯에 있는지 물어보십시오 :`solana --url http://127.0.0.1:8899 slot`. 80 % 지분에 도달 할 때까지`SLOT_X`를 반환해야합니다.

감사!

### 7 단계. 기다렸다가 들으세요

다시 시작할 때 유효성 검사기를 모니터링합니다. 질문에 답하고, 사람들을 돕고,
