---
title: 투표 계정 관리
---

이 페이지는 온 체인 *vote 계정 *을 설정하는 방법을 설명합니다. Solana에서 유효성 검사기 노드를 실행하려면 투표 계정을 만들어야합니다.

## 투표 계정

생성 \[create-vote-account\] (../ cli / usage.md # solana-create-vote-account) 명령으로 투표 계정을 생성 할 수 있습니다. 투표 계정은 처음 만들 때 또는 유효성 검사기가 실행 된 후에 구성 할 수 있습니다. \[투표 계정 주소\] (# vote-account-address)를 제외하고는 투표 계정의 모든 측면을 변경할 수 있으며, 이는 계정 수명 동안 고정됩니다.

### 기존 투표 계정 구성

- -\[validator identity\] (# validator-identity)를 변경하려면 \[vote-update-validator\] (../ cli / usage.md # solana-vote-update-validator)를 사용합니다.
- -\[탈퇴 권한\] (# withdraw-authority)을 변경하려면 \[vote-authorize-withdrawer\] (../ cli / usage.md # solana-vote-authorize-withdrawer)를 사용하세요.
- 인출 권한은 나중에 \[vote-authorize-withdrawer\] (../ cli / usage.md # solana-vote-authorize-withdrawer) 명령으로 변경할 수 있습니다.
- 커미션은 나중에 \[vote-update-commission\] (../ cli / usage.md # solana-vote-update-commission) 명령으로 변경할 수도 있습니다.

## 투표 계정 구조

### 투표 계정 주소

A vote account is created at an address that is either the public key of a keypair file, or at a derived address based on a keypair file's public key and a seed string.

투표 계정은 키 쌍 파일의 공개 키인 주소 또는 키 쌍 ​​파일의 공개 키와 시드 문자열을 기반으로하는 파생 주소에서 생성됩니다.

투표 계정의 주소는 거래에 서명하는 데 필요하지 않으며 계정 정보를 조회하는 데만 사용됩니다.

### Validator Identity

*validator identity*는 투표 계정에 제출 된 모든 투표 거래 수수료를 지불하는 데 사용되는 시스템 계정입니다. 밸리데이터는 수신 한 대부분의 유효한 블록에 투표 할 것으로 예상되기 때문에 밸리데이터 신원 계정은 자주 (잠재적으로 초당 여러 번) 거래에 서명하고 수수료를 지불합니다. 이러한 이유로 유효성 검사기 ID 키 쌍은 유효성 검사기 프로세스가 실행중인 동일한 시스템의 키 쌍 파일에 "핫 지갑"으로 저장되어야합니다.

핫 지갑은 일반적으로 오프라인 또는 "콜드"지갑보다 안전하지 않기 때문에 유효성 검사기 운영자는 몇 주 또는 몇 달과 같은 제한된 시간 동안 투표 수수료를 충당 할 수있는 충분한 SOL 만 ID 계정에 저장하도록 선택할 수 있습니다. 밸리데이터 신원 계정은 더 안전한 지갑에서 주기적으로 충전 될 수 있습니다.

이 방법은 유효성 검사기 노드의 디스크 또는 파일 시스템이 손상되거나 손상 될 경우 자금 손실 위험을 줄일 수 있습니다.

투표 계정을 만들 때 유효성 검사기 ID를 제공해야합니다. \[vote-update-validator\] (../ cli / usage.md # solana-vote-update-validator) 명령을 사용하여 계정을 만든 후에 밸리데이터 ID를 변경할 수도 있습니다.

### 투표 권한

_vote Authority_ 키 쌍은 유효성 검사기 노드가 클러스터에 제출하려는 각 투표 트랜잭션에 서명하는 데 사용됩니다. 이 문서의 뒷부분에서 볼 수 있듯이 유효성 검사기 ID에서 반드시 고유 할 필요는 없습니다. 유효성 검사기 ID와 마찬가지로 투표 권한은 트랜잭션에 자주 서명하기 때문에 유효성 검사기 프로세스와 동일한 파일 시스템의 핫 키 쌍이어야합니다.

투표 권한은 유효성 검사기 ID와 동일한 주소로 설정 될 수 있습니다. 밸리데이터 신원이 투표 권한이기도 한 경우, 투표에 서명하고 거래 수수료를 지불하기 위해 투표 거래 당 하나의 서명 만 필요합니다. Solana의 거래 수수료는 서명별로 평가되기 때문에 두 사람이 아닌 한 명의 서명자가 있으면 투표 권한과 유효성 검사기 ID를 두 개의 다른 계정으로 설정하는 것과 비교하여 지불되는 거래 수수료의 절반이됩니다.

투표 권한은 투표 계정이 생성 될 때 설정할 수 있습니다. 제공되지 않은 경우 기본 동작은 유효성 검사기 ID와 동일하게 할당하는 것입니다. 투표 권한은 나중에 \[vote-authorize-voter\] (../ cli / usage.md # solana-vote-authorize-voter) 명령으로 변경할 수 있습니다.

투표 권한은 Epoch 당 최대 한 번 변경할 수 있습니다. \[vote-authorize-voter\] (../ cli / usage.md # solana-vote-authorize-voter)로 권한이 변경되면 다음 Epoch가 시작될 때까지 적용되지 않습니다. 투표 서명의 원활한 전환을 지원하기 위해`solana-validator`는`--authorized-voter` 인수를 여러 번 지정할 수 있도록합니다. 이를 통해 네트워크가 유효성 검사자의 투표 권한 계정이 변경되는 기점 경계에 도달 할 때 유효성 검사 프로세스가 성공적으로 투표 할 수 있습니다.

### 권한 인출

_withdraw Authority_ 키 쌍은 \[withdraw-from-vote-account\] (../ cli / usage.md # solana-withdraw-from-vote-account) 명령을 사용하여 투표 계정에서 자금을 인출하는 데 사용됩니다. 밸리데이터가 획득 한 모든 네트워크 보상은 투표 계정에 입금되며 인출 권한 키 쌍으로 서명해야만 검색 할 수 있습니다.

The withdraw authority is also required to sign any transaction to change a vote account's [commission](#commission), and to change the validator identity on a vote account.

인출 권한은 또한 투표 계정의 \[commission\] (# commission)을 변경하고 투표 계정의 밸리데이터 ID를 변경하기 위해 모든 트랜잭션에 서명해야합니다.

인출 권한은 '--authorized-withdrawer'옵션으로 투표 계정 생성시 설정할 수 있습니다. 이것이 제공되지 않으면 유효성 검사기 ID가 기본적으로 철회 권한으로 설정됩니다.

The withdraw authority can be changed later with the [vote-authorize-withdrawer](../cli/usage.md#solana-vote-authorize-withdrawer) command.

### 커미션커미션

\_\_은 밸리데이터의 투표 계정에 입금 된 밸리데이터이 획득 한 네트워크 보상의 비율입니다. 나머지 보상은 각 스테이킹 계정의 활성 스테이킹 가중치에 비례하여 해당 투표 계정에 위임 된 모든 스테이킹 계정에 분배됩니다.

예를 들어, 투표 계정에 10 %의 커미션이있는 경우 특정 시대에 해당 밸리데이터가 얻은 모든 보상에 대해 이러한 보상의 10 %가 다음 세대의 첫 번째 블록에있는 투표 계정에 입금됩니다. 나머지 90 %는 즉시 활성 지분으로 위임 된 지분 계정에 예치됩니다.

밸리데이터은 더 낮은 커미션으로 인해 더 많은 보상이 위임자에게 전달되므로 더 많은 지분 위임을 유치하기 위해 낮은 커미션을 설정할 수 있습니다. 밸리데이터 노드를 설정하고 운영하는 데 드는 비용이 있기 때문에 밸리데이터은 최소한 비용을 충당 할만큼 충분히 높은 수수료를 설정하는 것이 이상적입니다.

'--commission'옵션으로 투표 계정 생성시 커미션을 설정할 수 있습니다. 제공되지 않으면 기본적으로 100 %로 설정되어 모든 보상이 투표 계정에 예치되고 위임 된 지분 계정에 전달되지 않습니다.

-\[commission\] (# commission)을 변경하려면 \[vote-update-commission\] (../ cli / usage.md # solana-vote-update-commission)을 사용합니다.

커미션을 설정할 때 [0-100] 세트의 정수 값만 허용됩니다. 정수는 커미션에 대한 백분율 포인트를 나타내므로`--commission 10 '으로 계정을 만들면 10 % 커미션이 설정됩니다.

## 키 순환순환

투표 계정 권한 키를하려면 라이브 유효성 검사기를 처리 할 때 특별한 처리가 필요합니다.

### 투표 계정 유효성

검사기 ID 유효성 검사기 ID를 변경하려면 투표 계정의 _withdraw Authority_ 키 쌍에 액세스해야합니다. 다음 단계에서는`~ / withdraw-authority.json`이 해당 키 쌍이라고 가정합니다.

1. 새 유효성 검사기 ID 키 쌍`solana-keygen new -o ~ / new-validator-keypair.json`을 만듭니다.
2. 새 ID 계정에`solana transfer ~ / new-validator-keypair.json 500`이 자금이 지원되었는지 확인합니다.
3. 3.`solana vote-update-validator ~ / vote-account-keypair.json ~ / new-validator-keypair.json ~ / withdraw-authority.json`을 실행하여 투표 계정에서 밸리데이터 ID를 수정합니다 .
4. 유효성 검사기를 다시 시작합니다. `--identity` 인수에 대한 새 ID 키 쌍 사용

### 투표 계정 승인 된 유권자

_vote authorization_ keypair는 epoch 경계에서만 변경할 수 있으며 원활한 마이그레이션을 위해`solana-validator`에 대한 몇 가지 추가 인수가 필요합니다.

1. 1.`solana epoch-info`를 실행합니다. 현재 에포크에 남은 시간이 많지 않다면 다음 에포크를 기다리는 것을 고려하여 밸리데이터이 다시 시작하고 따라 잡을 수 있도록 충분한 시간을 확보하십시오.
2. 새 투표 권한 키 쌍`solana-keygen new -o ~ / new-vote-authority.json`을 만듭니다.
3. Determine the current _vote authority_ keypair by running `solana vote-account ~/vote-account-keypair.json`. 유효성 검사기의 ID 계정 (기본값) 또는 다른 키 쌍일 수 있습니다. 다음 단계에서는`~ / validator-keypair.json`이 해당 키 쌍이라고 가정합니다.
4. 4.`solana vote-authorize-voter ~ / vote-account-keypair.json ~ / validator-keypair.json ~ / new-vote-authority.json`을 실행합니다. 새로운 투표 권한은 다음 세대부터 활성화 될 예정입니다.
5. 5.`solana-validator`는 이제 이전 및 새 투표 권한 키 쌍으로 다시 시작해야 다음 시대에 원활하게 전환 할 수 있습니다. Add the two arguments on restart: `--authorized-voter ~/validator-keypair.json --authorized-voter ~/new-vote-authority.json`
6. 클러스터가 다음 세대에 도달하면`-를 제거합니다. 이전 투표 권한 키 쌍이 더 이상 필요하지 않으므로 authorized-voter ~ / validator-keypair.json` 인수를 사용하고`solana-validator`를 다시 시작하십시오.

### 투표 계정 승인 인출 자

특별한 처리가 필요하지 않습니다. 필요에 따라`solana vote-authorize-withdrawer` 명령을 사용합니다.
