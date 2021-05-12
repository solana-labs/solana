---
title: "Transactions"
---

프로그램 실행은 클러스터에 제출되는 \[transaction\] (terminology.md # transaction)로 시작됩니다. Solana 런타임은 트랜잭션에 포함 된 각 \[instructions\] (terminology.md # instruction)을 순서대로 원자 적으로 처리하는 프로그램을 실행합니다.

## 트랜잭션 분석트랜잭션

이 섹션에서는의 이진 형식을 다룹니다.

### 트랜잭션 형식

트랜잭션은 서명의 \[compact-array\] (# compact-array-format)와 그 뒤에 \[message\] (# message-format)을 포함합니다. 서명 배열의 각 항목은 주어진 메시지의 \[디지털 서명\] (# signature-format)입니다. Solana 런타임은 서명 수가 \[메시지 헤더\] (# message-header-format)의 처음 8 비트에있는 번호와 일치하는지 확인합니다. 또한 각 서명이 메시지의 계정 주소 배열의 동일한 인덱스에있는 공개 키에 해당하는 개인 키로 서명되었는지 확인합니다.

#### 서명 형식

각 디지털 서명은 ed25519 바이너리 형식이며 64 바이트를 사용합니다.

### 메시지 형식

메시지에는 \[header\] (# message-header-format), \[계정 주소\] (# account-addresses-format)의 압축 배열, 최근 \[blockhash\] (# blockhash-format), \[instructions\] (# instruction-format)의 압축 배열이 뒤 따릅니다.

#### 메시지 헤더 형식

메시지 헤더에는 3 개의 부호없는 8 비트 값이 포함됩니다. 첫 번째 값은 포함하는 트랜잭션의 필수 서명 수입니다. 두 번째 값은 읽기 전용 인 해당 계정 주소의 수입니다. 메시지 헤더의 세 번째 값은 서명이 필요하지 않은 읽기 전용 계정 주소의 수입니다.

#### 계정 주소 형식따릅니다

서명이 필요한 주소는 계정 주소 배열의 시작 부분에 나타나며 쓰기 액세스를 먼저 요청하는 주소와 읽기 전용 계정이 뒤. 서명이 필요하지 않은 주소는 다시 한 번 읽기-쓰기 계정이 먼저이고 읽기 전용 계정이 뒤 따르는 주소를 따릅니다.

#### 블록 해시 형식

블록 해시는 32 바이트 SHA-256 해시를 포함합니다. 클라이언트가 원장을 마지막으로 관찰 한시기를 나타내는 데 사용됩니다. 밸리데이터은 블록 해시가 너무 오래되면 거래를 거부합니다.

### 명령어 형식

명령어에는 프로그램 ID 색인, 계정 주소 색인의 압축 배열, 불투명 한 8 비트 데이터의 압축 배열이 차례로 포함됩니다. 프로그램 ID 인덱스는 불투명 한 데이터를 해석 할 수있는 온 체인 프로그램을 식별하는 데 사용됩니다. 프로그램 ID 색인은 메시지의 계정 주소 배열에있는 계정 주소에 대한 서명되지 않은 8 비트 색인입니다. 계정 주소 인덱스는 각각 동일한 배열에 대한 부호없는 8 비트 인덱스입니다.

### Compact-Array 형식

compact-array는 각 배열 항목이 뒤에 오는 배열 길이로 직렬화됩니다. 배열 길이는 compact-u16이라는 특수한 다중 바이트 인코딩입니다.

#### Compact-u16 형식

compact-u16은 16 비트의 다중 바이트 인코딩입니다. 첫 번째 바이트는 하위 7 비트 값의 하위 7 비트를 포함합니다. 값이 0x7f보다 크면 상위 비트가 설정되고 값의 다음 7 비트가 두 번째 바이트의 하위 7 비트에 배치됩니다. 값이 0x3fff보다 크면 상위 비트가 설정되고 값의 나머지 2 비트는 세 번째 바이트의 하위 2 비트에 배치됩니다.

### 계정 주소 형식

계정 주소는 32 바이트의 임의 데이터입니다. 주소에 디지털 서명이 필요한 경우 런타임은이를 ed25519 키 쌍의 공개 키로 해석합니다.

## 명령어

각 \[instruction\] (terminology.md # instruction)은 단일 프로그램, 프로그램에 전달되어야하는 트랜잭션 계정의 하위 집합 및 프로그램에 전달되는 데이터 바이트 배열을 지정합니다. 이 프로그램은 데이터 배열을 해석하고 지침에 지정된 계정에서 작동합니다. 프로그램이 성공적으로 또는 오류 코드를 반환 할 수 있습니다. 오류 반환으로 인해 전체 트랜잭션이 즉시 실패합니다.

프로그램은 일반적으로 지원하는 명령을 구성하는 도우미 기능을 제공합니다. 예를 들어, 시스템 프로그램은 [`SystemInstruction :: CreateAccount`] (https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs # L63) 명령 :

```rust
pub fn create_account (
    from_pubkey : & Pubkey,
    to_pubkey : & Pubkey,
    lamports : u64,
    space : u64,
    owner : & Pubkey,
)-> Instruction {
    let account_metas = vec! [
        AccountMeta :: new (* from_pubkey, true),
        AccountMeta :: new (* to_pubkey, true),
    ];
    Instruction :: new (
        system_program :: id (),
        & SystemInstruction :: CreateAccount {
            lamports,
            space,
            owner : * owner,
        },
        account_metas,
    )
}
```

여기에서 찾을 수 있습니다 :

https://github.com/solana -labs / solana / blob / 6606590b8132e56dab9e60b3f7d20ba7412a736c / sdk / program / src / system_instruction.rs # L220

### 프로그램 Id

명령어의 \[프로그램 ID\] (terminology.md # program-id)는이 명령어를 처리 할 프로그램을 지정합니다. 프로그램의 계정 소유자는 프로그램을로드하고 실행하는 데 사용할 로더를 지정하며 데이터에는 런타임이 프로그램을 실행하는 방법에 대한 정보가 포함됩니다.

\[배포 된 BPF 프로그램\] (developing / deployed-programs / overview.md)의 경우 소유자는 BPF 로더이고 계정 데이터는 BPF 바이트 코드를 보유합니다. 프로그램 계정은 성공적으로 배포되면 로더에 의해 영구적으로 실행 가능한 것으로 표시됩니다. 런타임은 실행 불가능한 프로그램을 지정하는 트랜잭션을 거부합니다.

배포 된 프로그램과 달리 \[builtins\] (developing / builtins / programs.md)는 Solana 런타임에 직접 빌드된다는 점에서 다르게 처리됩니다.

### 계정

명령에서 참조하는 계정은 온 체인 상태를 나타내며 프로그램의 입력 및 출력 역할을합니다. 계정에 대한 자세한 내용은 \[계정\] (accounts.md) 섹션에서 찾을 수 있습니다.

### 명령어 데이터

각 명령어는 계정과 함께 프로그램에 전달되는 범용 바이트 배열을 가지고 있습니다. 명령 데이터의 내용은 프로그램에 따라 다르며 일반적으로 프로그램이 수행해야하는 작업과 계정에 포함 된 것 이상으로 이러한 작업에 필요할 수있는 추가 정보를 전달하는 데 사용됩니다.

프로그램은 정보가 명령어 데이터 바이트 배열로 인코딩되는 방법을 자유롭게 지정할 수 있습니다. 데이터가 인코딩되는 방법을 선택할 때 디코딩 오버 헤드를 고려해야합니다. 해당 단계는 온 체인 프로그램에 의해 수행되기 때문입니다. 일부 일반적인 인코딩 (예 : Rust의 bincode)은 매우 비효율적 인 것으로 관찰되었습니다.

\[Solana 프로그램 라이브러리의 토큰 프로그램\] (https://github.com/solana-labs/solana-program-library/tree/master/token)은 명령 데이터를 효율적으로 인코딩 할 수있는 방법에 대한 한 가지 예를 제공하지만이 방법에 유의하십시오. 토큰은 \[Pack\] (https://github.com/solana-labs/solana/blob/master/sdk/program/src/program_pack.rs) 특성을 활용하여 토큰 명령과 토큰 모두에 대한 명령 데이터를 인코딩 / 디코딩합니다. 계정 상태.

## 서명

각 트랜잭션은 트랜잭션 지침에서 참조하는 모든 계정 공개 키를 명시 적으로 나열합니다. 이러한 공개 키의 하위 집합은 각각 트랜잭션 서명과 함께 제공됩니다. 이러한 서명은 계정 소유자가 거래를 승인했다는 온 체인 프로그램을 나타냅니다. 일반적으로 프로그램은 권한을 사용하여 계정에서 인출 또는 데이터 수정을 허용합니다. 권한이 프로그램에 전달되는 방법에 대한 자세한 내용은 \[계정\] (accounts.md # signers)에서 찾을 수 있습니다.

## 최근 블록 해시

트랜잭션에는 중복을 방지하고 제공하기 위해 최근 \[blockhash\] (terminology.md # blockhash)가 포함됩니다. 거래 수명. 이전 트랜잭션과 완전히 동일한 트랜잭션은 거부되므로 새로운 블록 해시를 추가하면 여러 트랜잭션이 정확히 동일한 작업을 반복 할 수 있습니다. 또한 블록 해시가 너무 오래된 트랜잭션은 거부되므로 트랜잭션에는 블록 해시에 의해 정의 된 수명이 있습니다.
