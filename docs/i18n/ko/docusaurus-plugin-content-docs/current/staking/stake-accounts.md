---
title: 스테이킹 계정 구조
---

Solana의 스테이킹 계정을 사용하여 토큰을 네트워크의 밸리데이터에게 위임하여 스테이킹 계정 소유자에 대한 보상을받을 수 있습니다. 스테이킹 계정은 * 시스템 계정 *으로 알려진 기존 지갑 주소와 다르게 생성되고 관리됩니다. 시스템 계정은 네트워크의 다른 계정에서만 SOL을 보내고받을 수있는 반면, 스테이킹 계정은 토큰 위임을 관리하는 데 필요한 더 복잡한 작업을 지원합니다.

Solana의 스테이킹 계정은 익숙한 다른 Proof-of-Stake 블록체인 네트워크와는 다르게 작동합니다. 이 문서는 Solana 스테이킹 계정의 높은 수준의 구조와 기능을 설명합니다.

#### 계정 주소

각 스테이킹 계정에는 명령 줄 또는 네트워크 탐색기 도구에서 계정 정보를 조회하는 데 사용할 수있는 고유 한 주소가 있습니다. 그러나 주소의 키 쌍 소유자가 지갑을 제어하는 ​​지갑 주소와 달리 스테이킹 계정 주소와 관련된 키 쌍이 반드시 계정을 제어 할 수있는 것은 아닙니다. 실제로 스테이킹 계정의 주소에 대해 키 쌍 또는 개인 키가 존재하지 않을 수도 있습니다.

스테이킹 계정의 주소에 키 페어 파일이있는 유일한 경우는 \[명령 줄 도구를 사용하여 스테이킹 계정 생성\] (../ cli / delegate-stake.md # create-a-stake-account) 때 새 키 페어 파일이 스테이킹 계정의 주소가 새롭고 고유한지 확인하기 위해 먼저 생성되었습니다.

#### 계정 권한 이해

특정 유형의 계정에는 특정 계정과 관련된 하나 이상의 * 서명 기관 *이있을 수 있습니다. 계정 권한은 제어하는 ​​계정에 대한 특정 거래에 서명하는 데 사용됩니다. 이것은 계정의 주소와 관련된 키 쌍의 소유자가 모든 계정의 활동을 제어하는 ​​다른 블록체인 네트워크와 다릅니다.

각 스테이킹 계정에는 각각의 주소로 지정된 두 개의 서명 권한이 있으며, 각각은 스테이킹 계정에서 특정 작업을 수행 할 권한이 있습니다.

The _stake authority_ is used to sign transactions for the following operations:

- Delegating stake
- Deactivating the stake delegation
- Splitting the stake account, creating a new stake account with a portion of the funds in the first account
- Merging two undelegated stake accounts into one
- Setting a new stake authority

인출 권한 키 쌍은 스테이킹 계정의 토큰을 청산하는 데 필요하므로 계정에 대한 더 많은 제어 권한을 보유하며 스테이킹 권한 키 쌍이 분실되거나 손상 될 경우 스테이킹 권한을 재설정하는 데 사용할 수 있습니다.

- Withdrawing un-delegated stake into a wallet address
- Setting a new withdraw authority
- Setting a new stake authority

스테이킹 권한 및 철회 권한은 스테이킹 계정이 생성 될 때 설정되며 언제든지 새 서명 주소를 승인하도록 변경할 수 있습니다. 스테이킹 및 인출 권한은 동일한 주소이거나 두 개의 다른 주소 일 수 있습니다.

The withdraw authority keypair holds more control over the account as it is needed to liquidate the tokens in the stake account, and can be used to reset the stake authority if the stake authority keypair becomes lost or compromised.

이는 일부 토큰이 포함 된 지갑 주소에서 여러 지분 계정을 생성하거나 단일 대형 지분 계정을 생성하고 지분 권한을 사용하여 선택한 토큰 잔액이있는 여러 계정으로 계정을 분할하여 수행 할 수 있습니다.

#### 여러 위임

각 스테이킹 계정은 한 번에 한 명의 밸리데이터에게만 위임 할 수 있습니다. 계정의 모든 토큰은 위임 또는 비 위임 또는 위임 또는 비 위임 과정에 있습니다. 토큰의 일부를 밸리데이터에게 위임하거나 여러 밸리데이터에게 위임하려면 여러 스테이킹 계정을 생성해야합니다.

위임되지 않고 동일한 권한과 락업을 가진 두 개의 스테이킹 계정을 하나의 결과 스테이킹 계정으로 병합 할 수 있습니다.

스테이킹 계정이 위임되거나 위임이 비활성화되면 작업이 즉시 적용되지 않습니다.

위임 또는 비활성화는 완료하는 데 여러 \[epochs\] (../ terminology.md # epoch)가 필요하며, 명령이 포함 된 트랜잭션이 클러스터에 제출 된 후 위임의 일부가 각 Epoch 경계에서 활성화되거나 비활성화됩니다.

#### 위임 워밍업 및 쿨 다운

When a stake account is delegated, or a delegation is deactivated, the operation does not take effect immediately.

스테이킹 계정은 특정 날짜 또는 시대에 도달하기 전에 보유하고있는 토큰이 인출되는 것을 방지하는 락업을 가질 수 있습니다.

또한 네트워크 전체에서 지분이 갑작스럽게 크게 변경되는 것을 방지하기 위해 단일 에포크에서 총 지분이 얼마나 위임되거나 비활성화 될 수 있는지에 대한 제한이 있습니다. 워밍업 및 쿨 다운은 다른 네트워크 참여자의 행동에 따라 달라 지므로 정확한 기간을 예측하기가 어렵습니다. 준비 및 쿨 다운 타이밍에 대한 자세한 내용은 \[여기\] (../ cluster / stake-delegation-and-rewards.md # stake-warmup-cooldown-withdrawal)에서 확인할 수 있습니다.

#### 락업

Stake accounts can have a lockup which prevents the tokens they hold from being withdrawn before a particular date or epoch has been reached. While locked up, the stake account can still be delegated, un-delegated, or split, and its stake and withdraw authorities can be changed as normal. Only withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be modified later, by the _lockup authority_ or _custodian_, the address of which is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a balance of 0 SOL is no longer tracked. If a stake account is not delegated and all of the tokens it contains are withdrawn to a wallet address, the account at that address is effectively destroyed, and will need to be manually re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the Solana Explorer by copying and pasting an account address into the search bar.

- http://explorer.solana.com/accounts
