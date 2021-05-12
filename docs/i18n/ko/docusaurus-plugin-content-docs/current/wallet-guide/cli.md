---
title: Command-line Guide
---

In this section, we will describe how to use the Solana command-line tools to create a _wallet_, to send and receive SOL tokens, and to participate in the cluster by delegating stake.

** 명령 줄 프로그램 사용에 익숙하지 않고 SOL 토큰을주고 받기만하려는 경우 타사 \[앱 지갑\] (apps.md) **를 설정하는 것이 좋습니다.

To get started using the Solana Command Line (CLI) tools:

## Getting Started

_ 파일 시스템 지갑 _, 일명 FS 지갑은 컴퓨터 파일 시스템의 디렉토리입니다. 디렉토리의 각 파일에는 키 쌍이 있습니다.

### 파일 시스템 지갑 보안

파일 시스템 지갑은 가장 편리하고 가장 안전한 지갑입니다. 키 쌍이 간단한 파일에 저장되기 때문에 편리합니다. 원하는만큼의 키를 생성하고 파일을 복사하여 간단하게 백업 할 수 있습니다. 키 쌍 파일이 ** 암호화되지 않았기 때문에 ** 안전하지 않습니다. 컴퓨터의 유일한 사용자이고 맬웨어가 없다고 확신하는 경우 FS 지갑은 소량의 암호 화폐를위한 훌륭한 솔루션입니다. 그러나 컴퓨터에 맬웨어가 포함되어 있고 인터넷에 연결되어있는 경우 해당 맬웨어는 키를 업로드하고이를 사용하여 토큰을 가져올 수 있습니다. 마찬가지로 키 쌍이 컴퓨터에 파일로 저장되기 때문에 컴퓨터에 물리적으로 액세스 할 수있는 숙련 된 해커가 액세스 할 수 있습니다. MacOS의 FileVault와 같은 암호화 된 하드 드라이브를 사용하면 이러한 위험을 최소화 할 수 있습니다.

[File System Wallet](file-system-wallet.md)

## Create a Wallet

* 종이 지갑 *은 종이에 쓰여진 _ 종자 문구 _ 모음입니다. 시드 구문은 요청시 키 쌍을 재생성하는 데 사용할 수있는 몇 가지 단어 (일반적으로 12 또는 24)입니다.

### 종이 지갑 보안

편의성 대 보안 측면에서 종이 지갑은 FS 지갑과 반대편에 있습니다. 사용하기 매우 불편하지만 보안 성이 뛰어납니다. \[오프라인 서명\] (../ offline-signing.md)과 함께 종이 지갑을 사용하면 이러한 높은 보안이 더욱 강화됩니다. \[Coinbase Custody\] (https://custody.coinbase.com/)와 같은 보관 서비스는이 조합을 사용합니다. 종이 지갑과 보관 서비스는 오랜 기간 동안 많은 수의 토큰을 보호하는 훌륭한 방법입니다.

[Paper Wallets](paper-wallet.md)

## 하드웨어 지갑

하드웨어 지갑은 키 쌍을 저장하고 트랜잭션 서명을위한 인터페이스를 제공하는 소형 핸드 헬드 장치입니다.

### 하드웨어 지갑 보안

\[Ledger hardware wallet\] (https://www.ledger.com/)과 같은 하드웨어 지갑은 암호 화폐에 대한 보안과 편리함을 훌륭하게 결합합니다. 파일 시스템 지갑의 거의 모든 편의성을 유지하면서 오프라인 서명 프로세스를 효과적으로 자동화합니다.

[Hardware Wallets](hardware-wallets.md)
