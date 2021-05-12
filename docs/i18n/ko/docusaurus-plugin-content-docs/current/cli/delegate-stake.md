---
title: 위임 스테이킹
---

[SOL](transfer-tokens.md)을 받은 이후 *지분*을 밸리데이터에게 위임하여 사용하는것을 고려할 수 있습니다. 스테이킹은 *stake account*의 토큰을 말하는 것입니다. 솔라나의 검증인은 지분에 따라 투표합니다. 이를 통해 검증인이 결정하는데 더 많은 영향력을 행사할 수 있습니다. 그리고 블록체인에서 다음으로 유효한 트랜잭션 블록 결정에 더 큰 영향을 미칩니다. 이후 솔라나는 스테이커와 밸리데이터에게 보상하기 위해 주기적으로 새로운 SOL을 생성합니다. 더 많이 스테이킹할수록 더 많은 보상을 얻을 수 있습니다.

## 스테이킹 계정 생성

지분을 위임하려면 토큰을 스테이킹 계정으로 이전해야합니다. 스테이킹 계정을 생성하기 위해서는, 키 쌍이 필요합니다. 공개 키는 [ 스테이킹 계정 주소 ](../staking/stake-accounts.md#account-address)로 쓰입니다. 여기에서는 암호나 암호화가 필요하지 않습니다. 스테이킹 계정을 만든 후 키 쌍은 바로 삭제됩니다.

```bash
solana-keygen new --no-passphrase -o stake-account.json
```

출력에는 `pubkey:` 뒤에 공개키가 표시됩니다.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

공개 키를 복사하고 안전하게 보관하세요. 이후 생성할 스테이킹 계정에 이 공개키가 필요합니다.

이제 스테이킹 계정을 만듭니다:

```bash
solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --fee-payer <KEYPAIR>
```

`<AMOUNT>` 토큰은 `<KEYPAIR>`으로부터 stake-account.json의 신규 스테이킹 공개키로 전송됩니다.

이제 stake-account.json 파일을 삭제할 수 있습니다. 추가 권한을 부여하려면 `-stake-authority` 또는 `-withdraw-authority` 키 쌍을 사용합니다. stake-account.json 이 아닙니다.

`solana stake-account` 명령어로 새 지분 계정을 확인합니다.

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

출력값은 다음과 같습니다:

```text
Total Stake: 5000 SOL
Stake account is undelegated
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

### 스테이킹 설정 및 권한 철회

[Stake and withdraw authorities](../staking/stake-accounts.md#understanding-account-authorities) 계정은 `--stake-authority`와 `--withdraw-authority`옵션으로부터 계정을 만들 때 설정할 수 있습니다. 또는 `solana stake-authorize`명령으로 이후에 생성할 수 있습니다. 예를 들어 새로운 스테이킹 권한을 설정하려면 아래 명령어를 실행하세요:

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

이러면 `<KEYPAIR>`의 기존 스테이킹 권한을 사용해 스테이킹 계정 `<STAKE_ACCOUNT_ADDRESS><STAKE_ACCOUNT_ADDRESS>`에 신규 권한 `<PUBKEY>`을 승인합니다.

### 고급: 스테이킹 계정 주소 도출

스테이킹을 위임 할 때 스테이킹 계정의 모든 토큰을 단일 밸리데이터에게 위임합니다. 여러 밸리데이터에게 위임하려면 여러 스테이킹 계정이 필요합니다. 각 계정에 대해 새 키 쌍을 만들고 해당 주소를 관리하는 것은 번거로울 수 있습니다. 다행히도 다음 명령어를 사용하여 스테이킹 주소를 도출 할 수 있습니다. `-seed`

```bash
solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>
```

`<STRING>` 은 최대 32 바이트의 임의 문자열이지만 일반적으로 파생 된 계정에 해당하는 숫자입니다. 첫 번째 계정은 "0"이고, 이후 "1"이며 이어져서 나갑니다. `<STAKE_ACCOUNT_KEYPAIR>`의 공개 키가 기본 주소 역할을 합니다. 이 명령은 기본 주소와 시드 문자열에서 새 주소를 파생합니다. 어떤 스테이킹 주소인지 확인하려면 `solana create-address-with-seed <code>를 사용하세요.</p>

<pre><code class="bash">solana create-address-with-seed --from <PUBKEY> <SEED_STRING> STAKE
`</pre>

`<PUBKEY>`은 `solana create-stake-account`로 옮겨간 `<STAKE_ACCOUNT_KEYPAIR>`의 공개키 입니다.

커맨드는 스테이킹 처리 중 `<STAKE_ACCOUNT_ADDRESS>`에 대해 쓸 수 있는 주소를 파생시킵니다.

## 스테이킹 위임

스테이킹를 밸리데이터에게 위임하려면 투표 계정 주소가 필요합니다. `solana validators` 명령어를 통해 모든 밸리데이터 목록과 투표에 대해 클러스터를 쿼리하여 찾습니다.

```bash
solana validators
```

각 행의 첫 번째 열에는 밸리데이터의 신원이 포함되고 두 번째 열은 투표 계정 주소입니다. 밸리데이터를 선택하고 해당 투표 계정인 `solana delegate-stake`을 사용합니다.

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

스테이킹 권한자인 `<KEYPAIR>`가 `<STAKE_ACCOUNT_ADDRESS>` 주소를 통해 계정에 대한 작업을 승인합니다. 스테이킹 토큰은 투표 계정 주소인 `<VOTE_ACCOUNT_ADDRESS>` 앞으로 위임됩니다.

스테이킹 위임 후 `solana stake-account`을 사용하여 변경 사항을 확인하십시오.

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

출력에 새 필드 "Delegated Stake"및 "Delegated Vote Account Address"가 표시됩니다. 출력은 다음처럼 표시됩니다:

```text
Total Stake: 5000 SOL
Credits Observed: 147462
Delegated Stake: 4999.99771712 SOL
Delegated Vote Account Address: CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1
Stake activates starting from epoch: 42
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

## 스테이킹 비활성화

위임 후 `solana deactivate-stake` 명령으로 지분을 위임 해제 할 수 있습니다.

```bash
solana deactivate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

스테이킹 권한자인 `<KEYPAIR>`는 `<STAKE_ACCOUNT_ADDRESS>`주소를 통해 계정에 대한 작업을 승인합니다.

스테이킹는 "쿨다운"하는 몇 에폭 정도의 시간이 걸린다는 점에 유의하세요. 쿨다운 기간에 지분을 위임하려는 시도는 실패 처리 됩니다.

## 스테이킹 철회

`solana withdraw-stake` 명령을 사용하여 스테이킹 계정에서 토큰을 철회합니다.

```bash
solana withdraw-stake --withdraw-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>`는 기존 스테이킹 계정, 스테이킹 권한자인 `<KEYPAIR>`는 철회 권한자이며, `<AMOUNT>`은 `<RECIPIENT_ADDRESS>`에게 넘어가는 토큰 총액입니다.

## 분할 스테이킹

스테이킹을 출금 할 수 없을 동안 유저는 추가적인 밸리데이터에 대한 위임을 원할 수도 있습니다. 현재 스테이킹 중이거나, 쿨다운 또는 잠금 상태이기 때문에 그럴수 없을 수 있습니다. 기존 스테이킹 토큰을 기존 계정에서 신규 계정으로 바꾸려면 `solana split-stake` 명령을 사용합니다.

```bash
solana split-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>`는 기존 스테이킹 계정, 스테이킹 권한자는 `<KEYPAIR>`.`<NEW_STAKE_ACCOUNT_KEYPAIR>`는 새 계정의 키 쌍, `<AMOUNT>`는 전송할 토큰 수입니다.

스테이킹 계정을 파생 계정 주소로 분할하려면 `-seed`를 사용하십시오. 자세한 내용은. [ 스테이킹 계정 주소 만들기](#advanced-derive-stake-account-addresses)를 참조하세요.
