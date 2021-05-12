---
title: Ledger Nano
---

이 페이지에서는 Ledger Nano S 또는 Nano X를 사용하여 명령 줄 도구를 사용하여 Solana와 상호 작용하는 방법을 설명합니다.  Nano와 Solana와 상호 작용하는 다른 솔루션을 보려면 \[여기를 클릭하세요\] (../ ledger-live.md # interact-with-the-solana-network).

## 시작하기 전에

- [Set up a Nano with the Solana App](../ledger-live.md)
- [Install the Solana command-line tools](../../cli/install-solana-cli-tools.md)

## Solana CLI와 함께 Ledger Nano 사용

1. Ledger Live 애플리케이션이 닫혀 있는지 확인합니다 .
2. Nano를 컴퓨터의 USB 포트에 연결합니다.
3. 핀을 입력하고 Nano에서 Solana 앱을 시작합니다 .
4. 화면을 확인합니다. 읽고"응용 프로그램이

### 지갑 주소보기

-\[Solana 앱으로 Nano 설정\] (../ ledger-live.md) -\[Solana 명령 줄 도구 설치\] (../../ cli / install-solana-cli -tools.md)

```bash
```bash는
~ $ 솔라나 - Keygen은 pubkey의 USB : // 원장 키 = 42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
```

확인한다이 원장 장치가 제대로 연결되어 상호 작용에 대한 올바른 상태에있다 Solana CLI로. 이 명령은 원장의 고유 한 _wallet ID_를 반환합니다. 동일한 컴퓨터에 여러 개의 Nano 장치가 연결되어있는 경우 지갑 ID를 사용하여 사용할 Ledger 하드웨어 지갑을 지정할 수 있습니다. 컴퓨터에서 한 번에 하나의 Nano 만 사용하려는 경우 지갑 ID를 포함 할 필요가 없습니다. 특정 원장을 사용하기 위해 지갑 ID를 사용하는 방법은 \[여러 하드웨어 지갑 관리\] (# manage-multiple-hardware-wallets)를 참조하세요.

### Nano에서 SOL 보내기 Nano

Nano는 임의의 수의 유효한 지갑 주소와 서명자를 지원합니다. 주소를 보려면 아래와 같이`solana-keygen pubkey` 명령을 사용하고 그 뒤에 유효한 \[keypair URL\] (../ hardware-wallets.md # specify-a-keypair-url)을 입력하세요.

준비"`bash는
솔라 - Keygen은 pubkey의 USB를 : //
원장`이

다음 명령은 모두 주어진 키 쌍 경로와 관련된 다른 주소를 표시합니다. 시도해보십시오!

```bash
solana-keygen pubkey usb://ledger
solana-keygen pubkey usb://ledger?key=0
solana-keygen pubkey usb://ledger?key=1
solana-keygen pubkey usb://ledger?key=2
```

* NOTE: keypair url parameters are ignored in **zsh** &nbsp;[see troubleshooting for more info](#troubleshooting)

당신은뿐만 아니라`= 키`후 번호를 다른 값을 사용할 수 있습니다. 이 명령으로 표시되는 주소는 유효한 Solana 지갑 주소입니다. 각 주소와 관련된 비공개 부분은 Nano에 안전하게 저장되며이 주소에서 거래에 서명하는 데 사용됩니다. 토큰을받는 데 사용할 주소를 파생하는 데 사용한 키 쌍 URL을 기록해 두십시오.

장치에서 단일 주소 / 키 쌍만 사용하려는 경우 'key = 0'에있는 주소를 사용하는 것이 기억하기 쉬운 좋은 방법 일 수 있습니다. 로이 주소를보기

```bash
key = 0
solana-keygen pubkey usb : // ledger?
```

예를 들어 다른 목적으로 자신의 계정간에 토큰을 전송하거나 장치의 다른 키 쌍을 스테이킹 계정의 서명 기관으로 사용하려는 경우 여러 지갑 주소가 유용 할 수 있습니다.

### 여러 하드웨어 지갑 관리 여러 하드웨어 지갑의

잔액에관계없이 사용하는 지갑은`솔라 balance` 명령을 사용의 모든 계정의 잔액을

```bash
:<code>bash는
솔라 균형
SOME_WALLET_ADDRESS</code>예를
```

예를

```bash
~ $ 솔라 균형 CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL
```

참고 : 키 쌍 URL 매개 변수가 ** zsh을 무시하는 ** & NBSP; (# 문제 해결) [추가 정보를 원하시면 문제 해결을 참조하십시오]

참고 : 원장에 새로 생성 된 주소와 같이 잔액이 0 SOL 인 주소는 탐색기에서 "찾을 수 없음"으로 표시됩니다. 빈 계정과 존재하지 않는 계정은 Solana에서 동일하게 취급됩니다. 계정 주소에 SOL이 있으면 변경됩니다.

### zsh에서 키 쌍 URL 매개 변수가 무시됩니다.

에서 제어하는 ​​주소에서 일부 토큰을 보내려면 주소를 파생하는 데 사용한 것과 동일한 키 쌍 URL을 사용하여 장치를 사용하여 트랜잭션에 서명해야합니다. 이렇게하려면 Nano가 연결되어 있고 PIN으로 잠금 해제되었는지, Ledger Live가 실행되고 있지 않은지, 그리고 Solana 앱이 장치에서 열려 있고 "Application is Ready"가 표시되는지 확인하십시오.

수신 역할을 지갑 주소 (또는 여러 개의 주소), 공개적으로이 주소 중 하나를 공유 할 수  해당 주소의 트랜잭션에 대한 서명자로 연관된 키 쌍 URL을 사용할 수 있습니다.

```bash
``KEYPAIR_URL_OF_SENDER --keypair`bash는
솔라 전송 RECIPIENT_ADDRESS
금액은```다음은
```

전체 예제입니다. 먼저 특정 키 쌍 URL에서 주소가 표시됩니다. 둘째, 주소의 잔액을 확인합니다. 마지막으로 '1'SOL을받는 사람 주소 '7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri'로 전송 트랜잭션이 입력됩니다. 전송 명령에 대해 Enter 키를 누르면 Ledger 장치에서 거래 세부 정보를 승인하라는 메시지가 표시됩니다. 기기에서 오른쪽 및 왼쪽 버튼을 사용하여 거래 세부 정보를 검토합니다. 올바르게 보이면 "승인"화면에서 두 버튼을 클릭하고, 그렇지 않으면 "거부"화면에서 두 버튼을 모두 누르십시오.

```bash
~$ solana-keygen pubkey usb://ledger?key=42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://ledger?key=42
Waiting for your approval on Ledger hardware wallet usb://ledger/2JT2Xvy6T8hSmT8g6WdeDbHUgoeGdj6bE2VueCZUJmyN
✅ Approved

Signature: kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

거래를 승인 한 후, 프로그램이 트랜잭션 서명을 표시하고, 반환하기 전에 확인서의 최대 번호 (32)를 기다립니다. 이 작업은 몇 초 밖에 걸리지 않으며 Solana 네트워크에서 트랜잭션이 완료됩니다. \[Explorer\] (https://explorer.solana.com/transactions)의 거래 탭으로 이동하여 거래 서명을 붙여 넣으면이 거래 또는 기타 거래의 세부 정보를 볼 수 있습니다.

## 고급 작업

### Manage Multiple Hardware Wallets

키로 트랜잭션에 서명하는 것이 때때로 유용합니다. 여러 지갑으로 서명하려면 _ 정규화 된 키 쌍 URL_이 필요합니다. URL이 정규화되지 않은 경우 Solana CLI는 연결된 모든 하드웨어 지갑의 정규화 된 URL을 입력하라는 메시지를 표시하고 각 서명에 사용할 지갑을 선택하도록 요청합니다.

대화 형 프롬프트를 사용하는 대신 Solana CLI`resolve-signer` 명령을 사용하여 정규화 된 URL을 생성 할 수 있습니다. 예를 들어, 핀 잠금을 해제하고 다음 명령을 실행, USB에 나노를 연결을 시도

```text
solana resolve-signer usb://ledger?key=0/0
```

:`bash는
솔라 균형
7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`당신은

```text
:<code>텍스트
USB : // 원장 / BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK 키 = 0 /
0</code>하지만
```

:`bash는
솔라 균형
7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`당신은

정규화 된 URL을 사용하면 여러 하드웨어 지갑을 동일한 컴퓨터에 연결하고 그중 하나에서 키 쌍을 고유하게 식별 할 수 있습니다. `solana` 명령이`<KEYPAIR>`항목이 주어진 트랜잭션의 해당 부분에 대한 서명자로 해석 된 경로를 사용할 것으로 예상하는 곳이면 어디에서나`resolve-signer` 명령의 출력을 사용하십시오.

## 문제 해결

### Keypair URL parameters are ignored in zsh

물음표 문자는 zsh에서 특수 문자입니다. 그건 당신이 사용하는 기능이 아닌 경우, 다음 행을 추가하려면`~ / .zshrc` 일반 문자로 취급하는 방법

```bash
:<code>bash는
unsetopt
nomatch을</code>다음
```

중 쉘 창을 다시 시작하거나 실행`~ / .zshrc

```bash
````bash는소스
~ /
.zshrc```당신이
```

물음표 문자의 해제 zsh을 특별 취급하지 원하는 경우, 당신은 당신의 키 쌍 URL을 백 슬래시로 명시 적으로 비활성화 할 수 있습니다. 예를

```bash
solana-keygen pubkey usb://ledger\?key=0
```

## 지원은

다음

\[토큰 보내기 및 받기\] (../../ cli / transfer-tokens.md) 및 \[위임 지분\] (../../ cli / delegate-stake.md)에 대해 자세히 알아보세요. `<KEYPAIR>`를 허용하는 옵션 또는 인수가 표시되는 모든 곳에서 Ledger 키 쌍 URL을 사용할 수 있습니다.
