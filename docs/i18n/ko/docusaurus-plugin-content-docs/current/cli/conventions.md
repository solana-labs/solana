---
title: Solana CLI 사용법
---

Solana CLI 명령을 실행하기 전에 모든 명령에서 볼 수있는 몇 가지 규칙을 살펴 보겠습니다. 첫째, Solana CLI는 서로 다른 작업에 대한 명령 모음입니다. 다음 명령어를 통하여 실행 가능한 모든 명령어를 확인할 수 있습니다.

```bash
solana --help
```

특정 명령어를 사용하는 방법을 확인하려면 다음을 실행하세요:

```bash
solana <COMMAND> --help
```

특정 명령어를 입력하는 `<COMMAND>`에 원하는 명령어를 입력하면 해당 명령어 사용법이 나타납니다.

명령의 사용법 메시지에는 일반적으로 다음과 같은 단어가 포함됩니다. `<AMOUNT>`, `<ACCOUNT_ADDRESS>` 이나 `<KEYPAIR>`. 각 단어는 다음의 * 유형 *에 대한 명령을 실행할 수있는 텍스트입니다. 예를 들어 `<AMOUNT>`는 `42` 또는 `100.42`와 같은 숫자들로 대체 가능합니다. `<ACCOUNT_ADDRESS>`를 base58로 암호화된 여러분의 공개키로 대체할 수 있습니다. `9grmKMwTiZwUHSExjtbFzHLPTdWoXgcg1bZkhvwTrTww`와 같습니다.

## 키 쌍 규칙

CLI 도구를 사용하는 많은 명령에는 `<KEYPAIR>`에 대한 값이 필요합니다. 키 쌍에 써야할 값의 종류는 [command line wallet you created](../wallet-guide/cli.md)에 달려 있습니다.

예를 들어 지갑 주소(키 쌍의 퍼블릭 키) 를 표시하는 방법은 CLI 도움말 문서 에서 다음과 같이 표시되어 있습니다:

```bash
solana-keygen pubkey <KEYPAIR>
```

아래는 지갑 유형에 따라 `<KEYPAIR>`에 무엇을 입력하는지에 대한 해결입니다.

#### 종이 지갑

종이 지갑에서 키 쌍은 시드 단어와 지갑을 만들 때 입력한 암호로부터 안전하게 파생됩니다. 종이 지갑을 사용하려면 `<KEYPAIR>` 텍스트가 예시 또는 도움 문서에 나온 텍스트여야합니다. `ASK`라는 단어를 입력하면 프로그램이 명령을 실행할 때 시드 단어를 입력하라는 메시지를 표시합니다.

다음 명령어로 종이 지갑의 지갑 주소를 표시합니다:

```bash
solana-keygen pubkey ASK
```

#### 파일 시스템 지갑

파일 시스템 지갑을 사용하면 키 쌍이 컴퓨터의 파일에 저장됩니다. `<KEYPAIR>`를 키 쌍 파일을 향한 전체 파일 경로로 바꿉니다.

예를 들어 파일 시스템 키 쌍 파일 위치가 `/home/solana/my_wallet.json` 이면, 다음과 같은 명령어를 실행해 주세요:

```bash
solana-keygen pubkey /home/solana/my_wallet.json
```

#### 하드웨어 지갑

하드웨어 지갑을 선택한 경우 [keypair URL](../wallet-guide/hardware-wallets.md#specify-a-hardware-wallet-key), 예: `usb: // ledger? key = 0`.

```bash
solana-keygen pubkey usb://ledger?key=0
```
