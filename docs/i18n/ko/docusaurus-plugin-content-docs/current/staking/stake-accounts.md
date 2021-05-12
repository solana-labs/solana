---
title: 스테이킹 계정 구조
---

Solana의 스테이킹 계정을 사용하여 토큰을 네트워크의 밸리데이터에게 위임하여 스테이킹 계정 소유자에 대한 보상을받을 수 있습니다. Stake accounts are created and managed differently than a traditional wallet address, known as a _system account_. 시스템 계정은 네트워크의 다른 계정에서만 SOL을 보내고받을 수있는 반면, 스테이킹 계정은 토큰 위임을 관리하는 데 필요한 더 복잡한 작업을 지원합니다.

Solana의 스테이킹 계정은 익숙한 다른 Proof-of-Stake 블록체인 네트워크와는 다르게 작동합니다. 이 문서는 Solana 스테이킹 계정의 높은 수준의 구조와 기능을 설명합니다.

#### 계정 주소

각 스테이킹 계정에는 명령 줄 또는 네트워크 탐색기 도구에서 계정 정보를 조회하는 데 사용할 수있는 고유 한 주소가 있습니다. 그러나 주소의 키 쌍 소유자가 지갑을 제어하는 ​​지갑 주소와 달리 스테이킹 계정 주소와 관련된 키 쌍이 반드시 계정을 제어 할 수있는 것은 아닙니다. 실제로 스테이킹 계정의 주소에 대해 키 쌍 또는 개인 키가 존재하지 않을 수도 있습니다.

스테이킹 계정의 주소에 키 페어 파일이있는 유일한 경우는 \[명령 줄 도구를 사용하여 스테이킹 계정 생성\] (../ cli / delegate-stake.md # create-a-stake-account) 때 새 키 페어 파일이 스테이킹 계정의 주소가 새롭고 고유한지 확인하기 위해 먼저 생성되었습니다.

#### 계정 권한 이해

Certain types of accounts may have one or more _signing authorities_ associated with a given account. 계정 권한은 제어하는 ​​계정에 대한 특정 거래에 서명하는 데 사용됩니다. 이것은 계정의 주소와 관련된 키 쌍의 소유자가 모든 계정의 활동을 제어하는 ​​다른 블록체인 네트워크와 다릅니다.

각 스테이킹 계정에는 각각의 주소로 지정된 두 개의 서명 권한이 있으며, 각각은 스테이킹 계정에서 특정 작업을 수행 할 권한이 있습니다.

The _stake authority_ is used to sign transactions for the following operations:

- Delegating stake
- Deactivating the stake delegation
- Splitting the stake account, creating a new stake account with a portion of the funds in the first account
- Merging two stake accounts into one
- Setting a new stake authority

The _withdraw authority_ signs transactions for the following:

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

#### Merging stake accounts

Two stake accounts that have the same authorities and lockup can be merged into a single resulting stake account. A merge is possible between two stakes in the following states with no additional conditions:

- two deactivated stakes
- an inactive stake into an activating stake during its activation epoch

For the following cases, the voter pubkey and vote credits observed must match:

- two activated stakes
- two activating accounts that share an activation epoch, during the activation epoch

All other combinations of stake states will fail to merge, including all "transient" states, where a stake is activating or deactivating with a non-zero effective stake.

#### Delegation Warmup and Cooldown

When a stake account is delegated, or a delegation is deactivated, the operation does not take effect immediately.

A delegation or deactivation takes several [epochs](../terminology.md#epoch) to complete, with a fraction of the delegation becoming active or inactive at each epoch boundary after the transaction containing the instructions has been submitted to the cluster.

There is also a limit on how much total stake can become delegated or deactivated in a single epoch, to prevent large sudden changes in stake across the network as a whole. Since warmup and cooldown are dependent on the behavior of other network participants, their exact duration is difficult to predict. Details on the warmup and cooldown timing can be found [here](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Lockups

Stake accounts can have a lockup which prevents the tokens they hold from being withdrawn before a particular date or epoch has been reached. While locked up, the stake account can still be delegated, un-delegated, or split, and its stake and withdraw authorities can be changed as normal. Only withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be modified later, by the _lockup authority_ or _custodian_, the address of which is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a balance of 0 SOL is no longer tracked. If a stake account is not delegated and all of the tokens it contains are withdrawn to a wallet address, the account at that address is effectively destroyed, and will need to be manually re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the Solana Explorer by copying and pasting an account address into the search bar.

- http://explorer.solana.com/accounts
