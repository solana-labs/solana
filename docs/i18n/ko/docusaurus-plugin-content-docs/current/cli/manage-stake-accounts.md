---
title: 스테이킹 계정 관리
---

스테이킹를 여러 밸리데이터에게 위임하려면 각각에 대해 별도의 스테이킹 계정을 만들어야합니다. 첫 번째 지분 계정을 시드 "0"에, 두 번째는 "1"에, 세 번째는 "2"에 생성하는 규칙을 따르는 경우 `solana-stake-accounts` 도구로 한 번의 호출로 모든 계정에서 작업 할 수 있습니다. 이를 사용하여 모든 계정의 잔액을 합산하거나 계정을 새 지갑으로 이동하거나 새로운 권한을 설정할 수 있습니다.

## 사용법

### 스테이킹 계정 생성

스테이킹 권한 공개키에 파생 스테이킹 계정을 만들고 자금을 조달하기:

```bash
solana-stake-accounts new <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

### 계정수

파생 계정 수를 계산합니다.

```bash
solana-stake-accounts count <BASE_PUBKEY>

```

### 스테이킹 계정 잔액 합산

파생 된 스테이킹 계정의 잔액을 더합니다.

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### 스테이킹 계정 주소 얻기

주어진 공개키에서 파생 된 각 스테이킹 계정의 주소를 나열합니다.

```bash
solana-stake-accounts addresses <BASE_PUBKEY> --num-accounts <NUMBER>
```

### 새로운 권한 설정

파생된 각 스테이킹 계정에 새로운 권한을 설정합니다.

```bash
solana-stake-accounts authorize <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```

### 스테이킹 계정 재배치

스테이킹 계정 재배치:

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER> \
    --fee-payer <KEYPAIR>
```

소액 재조정 및 각 스테이킹 계정 권한 부여의 경우 'move' 명령을 사용합니다:

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```
