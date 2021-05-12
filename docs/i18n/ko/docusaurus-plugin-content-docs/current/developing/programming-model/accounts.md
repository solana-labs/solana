---
title: "Accounts"
---

## Storing State between Transactions

트랜잭션 간 상태 저장 프로그램이 트랜잭션 간 상태를 저장해야하는 경우 *accounts*를 사용합니다. 계정은 Linux와 같은 운영 체제의 파일과 유사합니다. 파일과 마찬가지로 계정은 임의의 데이터를 보유 할 수 있으며 해당 데이터는 프로그램의 수명 이후에도 유지됩니다. 또한 파일과 마찬가지로 계정에는 데이터에 액세스 할 수있는 사용자와 방법을 런타임에 알려주는 메타 데이터가 포함됩니다.

파일과 달리 계정에는 파일 수명 동안의 메타 데이터가 포함됩니다. 그 수명은 *lamports*라고하는 소수의 네이티브 토큰 인 "토큰"으로 표현됩니다. 계정은 밸리데이터 메모리에 보관되며 \[ "임대료"\] (# 임대료)를 지불하여 거기에 머물러 있습니다. 각 밸리데이터은 주기적으로 모든 계정을 스캔하고 임대료를 징수합니다. 램프 포트가 0으로 떨어지는 모든 계정은 제거됩니다. Accounts can also be marked [rent-exempt](#rent-exemption) if they contain a sufficient number of lamports.

Linux 사용자가 경로를 사용하여 파일을 찾는 것과 같은 방식으로 Solana 클라이언트는 *address*를 사용하여 계정을 찾습니다. 주소는 256 비트 공개 키입니다.

## 서명자

거래에는 거래에서 참조하는 계정의 공개 키에 해당하는 디지털 \[서명\] (terminology.md # signature)이 포함될 수 있습니다. 해당 디지털 서명이 있으면 계정의 개인 키 소유자가 서명하여 트랜잭션을 "승인"했음을 의미하며 계정은 *signer*라고합니다. 계정이 서명자인지 여부는 계정 메타 데이터의 일부로 프로그램에 전달됩니다. 그런 다음 프로그램은 해당 정보를 사용하여 권한 결정을 내릴 수 있습니다.

## 읽기 전용읽기

트랜잭션은 트랜잭션 간의 병렬 계정 처리를 가능하게하기 위해 참조하는 일부 계정이 *전용 계정 *으로 취급됨을 \[표시\] (transactions.md # message-header-format) 할 수 있습니다. 런타임을 사용하면 여러 프로그램에서 읽기 전용 계정을 동시에 읽을 수 있습니다. 프로그램이 읽기 전용 계정을 수정하려고하면 런타임에서 트랜잭션이 거부됩니다.

## 실행 가능

계정이 메타 데이터에서 "실행 가능"으로 표시된 경우 계정의 공개 키와 명령어의 \[프로그램 ID\] (transactions.md # program-id)를 포함하여 실행할 수있는 프로그램으로 간주됩니다. 계정을 소유 한 로더는 성공적인 프로그램 배포 프로세스 중에 계정을 실행 가능한 것으로 표시합니다. 예를 들어, BPF 프로그램 배포 중에 로더가 계정 데이터의 BPF 바이트 코드가 유효하다고 판단하면 로더는 프로그램 계정을 실행 가능한 것으로 영구적으로 표시합니다. 실행 후 런타임은 계정의 데이터 (프로그램)를 변경할 수 없도록 강제합니다.

## 생성생성

계정을하기 위해 클라이언트는 *keypair*를 생성하고 고정 스토리지 크기 (바이트)가 미리 할당 된`SystemProgram :: CreateAccount` 명령을 사용하여 공개 키를 등록합니다. 계정 데이터의 현재 최대 크기는 10MB입니다.

계정 주소는 임의의 256 비트 값이 될 수 있으며 고급 사용자가 파생 주소를 만드는 메커니즘이 있습니다 (`SystemProgram :: CreateAccountWithSeed`, [`Pubkey :: CreateProgramAddress`] (calling-between-programs.md # program-derived -구애)).

시스템 프로그램을 통해 생성 된 적이없는 계정도 프로그램에 전달할 수 있습니다. 명령어가 이전에 생성되지 않은 계정을 참조하는 경우 프로그램은 시스템 프로그램이 소유하고 램프 포트가 0이고 데이터가 0 인 계정을 전달합니다. 그러나 계정은 거래의 서명자인지 여부를 반영하므로 권한으로 사용할 수 있습니다. 이 컨텍스트의 권한은 계정의 공개 키와 관련된 개인 키 소유자가 거래에 서명했음을 프로그램에 전달합니다. 계정의 공개 키는 프로그램에 알려 지거나 다른 계정에 기록 될 수 있으며 프로그램이 제어하거나 수행하는 자산 또는 작업에 대한 일종의 소유권 또는 권한을 나타냅니다.

## 프로그램에소유권 및 할당

대한생성 된 계정은 시스템 프로그램이라는 기본 제공 프로그램에 의해 * 소유 *되도록 초기화되며 * 시스템 계정 *이라고합니다. 계정에는 "소유자"메타 데이터가 포함됩니다. 소유자는 프로그램 ID입니다. 런타임은 ID가 소유자와 일치하는 경우 프로그램에 계정에 대한 쓰기 액세스 권한을 부여합니다. 시스템 프로그램의 경우 런타임을 통해 클라이언트는 램프 포트 및 중요한 _assign_ 계정 소유권을 전송할 수 있습니다. 이는 소유자를 다른 프로그램 ID로 변경하는 것을 의미합니다. 계정이 프로그램 소유가 아닌 경우 프로그램은 데이터를 읽고 계정에 크레딧을 제공하는 것만 허용됩니다.

## Verifying validity of unmodified, reference-only accounts

For security purposes, it is recommended that programs check the validity of any account it reads but does not modify.

The security model enforces that an account's data can only be modified by the account's `Owner` program. Doing so allows the program to trust that the data passed to them via accounts they own will be in a known and valid state. The runtime enforces this by rejecting any transaction containing a program that attempts to write to an account it does not own. But, there are also cases where a program may merely read an account they think they own and assume the data has only been written by themselves and thus is valid. But anyone can issues instructions to a program, and the runtime does not know that those accounts are expected to be owned by the program. Therefore a malicious user could create accounts with arbitrary data and then pass these accounts to the program in the place of a valid account. The arbitrary data could be crafted in a way that leads to unexpected or harmful program behavior.

To check an account's validity, the program should either check the account's address against a known value or check that the account is indeed owned correctly (usually owned by the program itself).

One example is when programs read a sysvar. Unless the program checks the address or owner, it's impossible to be sure whether it's a real and valid sysvar merely by successful deserialization. Accordingly, the Solana SDK [checks the sysvar's validity during deserialization](https://github.com/solana-labs/solana/blob/a95675a7ce1651f7b59443eb146b356bc4b3f374/sdk/program/src/sysvar/mod.rs#L65).

If the program always modifies the account in question, the address/owner check isn't required because modifying an unowned (could be the malicious account with the wrong owner) will be rejected by the runtime, and the containing transaction will be thrown out.

## Rent

Solana의유지 계정은 클러스터가 향후 트랜잭션을 처리하기 위해 데이터를 적극적으로 유지해야하기 때문에 *rent*라는 스토리지 비용이 발생합니다. 이것은 계정을 저장하는 데 비용이 발생하지 않는 Bitcoin 및 Ethereum과 다릅니다.

임대료는 트랜잭션에 의해 현재 시대의 첫 번째 액세스 (초기 계정 생성 포함)시 런타임까지 계정 잔액에서 차감되거나 트랜잭션이없는 경우 한 시대 당 한 번 인출됩니다. 수수료는 현재 고정 된 비율이며 바이트-시간-에포크 단위로 측정됩니다. 수수료는 추후 변경 될 수 있습니다.

간단한 임대료 계산을 위해 임대료는 항상 단일, 전체 세대에 대해 징수됩니다. 임대료는 비례 배분되지 않습니다. 즉, 부분 세대에 대한 수수료 나 환불이 없습니다. 즉, 계정 생성시 수집 된 첫 번째 임대료는 현재 부분 시대에 대한 것이 아니라 다음 전체 시대에 대해 미리 징수된다는 것을 의미합니다. 이후의 임대료 징수는 더 많은 미래 시대를위한 것입니다. 반면에 이미 렌트 한 계정의 잔액이 에포크 중간에 다른 렌트비 아래로 떨어지면 해당 계정은 현재 에포크 동안 계속 존재하며 다가오는 에포크가 시작될 때 즉시 삭제됩니다.

계정이 최소 잔액을 유지하는 경우 임대료 지불에서 면제 될 수 있습니다. 이 임대료 면제는 아래에 설명되어 있습니다.

### 임대료 계산

참고 : 임대료는 향후 변경 될 수 있습니다.

글을 쓰는 시점에서 고정 임대료는 테스트 넷 및 메인 넷-베타 클러스터에서 바이트 시대 당 19.055441478439427 램프 포트입니다. \[epoch\] (terminology.md # epoch)는 2 일을 목표로합니다 (devnet의 경우 임대료는 54m36s 길이의 epoch로 바이트 시대 당 0.3608183131797095 lamports입니다).

이 값은 mebibyte-day 당 0.01 SOL을 목표로 계산됩니다 (mebibyte-year 당 3.56 SOL과 정확히 일치) :

```text
Rent fee : 19.055441478439427 = 10_000_000 (0.01 SOL) * 365 (약 1 년 중 하루) / ( 1024 * 1024) (1 MiB) / (365.25 / 2) (1 년에 에포크)
```

임대 계산은`f64` 정밀도로 수행되고 최종 결과는 램프 포트에서`u64`로 잘립니다.

임대료 계산에는 계정 크기의 계정 메타 데이터 (주소, 소유자, 램프 포트 등)가 포함됩니다. 따라서 임대 계산에 사용할 수있는 가장 작은 계정은 128 바이트입니다.

예를 들어, 10,000 개의 램프 포트의 초기 전송으로 계정이 생성되고 추가 데이터는 없습니다. 임대료는 생성시 즉시 인출되어 7,561 램프 포트의 잔액이됩니다.```

````text
활동이 없더라도 계정 잔액은 다음 세대에 5,122 램프 포트로 감소됩니다 :

```텍스트
계정 잔액 : 5,122 = 7,561 (현재 잔액 )-2,439 (이 계정의 한 시대에 대한 임대료)
````

텍스트 임대 : 2,439 = 19.055441478439427 (임대료) _ 128 바이트 (최소 계정 크기) _ 1 (에포크) 계정 잔액 : 7,561 = 10,000 ( 양도 된 램프 포트)-2,439 (이 계정의 한 시대에 대한 임대료)

```text
Account Balance: 5,122 = 7,561 (current balance) - 2,439 (this account's rent fee for an epoch)
```

따라서 양도 된 램프가 2,439보다 작거나 같은 경우 최소 크기 계정은 생성 후 즉시 제거됩니다.

### 임대료 면제

또는 최소 2 년 분량의 임대료를 예치하여 계정을 임대료 징수에서 완전히 면제 할 수 있습니다. 이는 계정 잔액이 감소 할 때마다 확인되며 잔액이 최소 금액 이하로 떨어지면 임대료가 즉시 인출됩니다.

프로그램 실행 계정은 제거되는 것을 방지하기 위해 런타임에서 임대 면제가 필요합니다.

참고 : 특정 계정 크기에 대한 최소 잔액을 계산하려면 [`getMinimumBalanceForRentExemption` RPC 엔드 포인트] (developing / clients / jsonrpc-api.md # getminimumbalanceforrentexemption)를 사용하세요. 다음 계산은 설명 용일뿐입니다.

예를 들어, 15,000 바이트 크기의 프로그램 실행 파일은 임대 면제를 위해 105,290,880 램프 포트 (= ~ 0.105 SOL)의 잔액이 필요

```text
합니다
.<code>text 105,290,880 = 19.055441478439427 (수수료) * (128 + 15_000) (계정 크기 메타 데이터 포함) * ((365.25 / 2) * 2) (2 년 후 에포크)</code>
```

Rent can also be estimated via the [`solana rent` CLI subcommand](cli/usage.md#solana-rent)

```text
$ solana rent 15000
Rent per byte-year: 0.00000348 SOL
Rent per epoch: 0.000288276 SOL
Rent-exempt minimum: 0.10529088 SOL
```

Note: Rest assured that, should the storage rent rate need to be increased at some point in the future, steps will be taken to ensure that accounts that are rent-exempt before the increase will remain rent-exempt afterwards
