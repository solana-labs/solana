---
title: 프로그램호출프로그램
---

## 프로그램 파생 주소

Solana 런타임을 사용하면 프로그램 간 호출이라는 메커니즘을 통해 프로그램이 서로 호출 할 수 있습니다.  프로그램 간의 호출은 다른 프로그램의 명령을 호출하는 한 프로그램에 의해 수행됩니다.  호출 프로그램은 호출 된 프로그램이 명령 처리를 완료 할 때까지 중지됩니다.

For example, a client could create a transaction that modifies two accounts, each owned by separate on-chain programs:

```rust,ignore
, ignore
let message = Message :: new (vec! [
    .```rusttoken_instruction :: pay (& alice_pubkey),
    acme_instruction : : launch_missiles (& bob_pubkey),
]);
client.send_and_confirm_message (& [& alice_keypair, & bob_keypair], & message);
```

예를 들어, 클라이언트는 각각 별도의 온 체인 프로그램이 소유 한 두 개의 계정을 수정하는 트랜잭션을 생성 할 수 있습니다

```rust,ignore
[
    acme_instruction :: pay_and_launch_missiles (& alice_pubkey, & bob_pubkey),
]);
client.send_and_confirm_message (& [& alice_keypair, & bob_keypair], & message);
```

두 개의 온 체인 프로그램`token`과`acme`가 주어지면 각각`pay ()`와`launch_missiles ()`명령어를 구현하는 각각의 명령어는`token` 모듈에 정의 된 함수를 호출하여 acme를 구현할 수 있습니다. 교차 프로그램 호출을 실행하여 :

```rust,ignore
mod acme {
    use token_instruction;

    fn launch_missiles (accounts : & [AccountInfo])-> 결과 <()> {
        ...
    }

    fn pay_and_launch_missiles (accounts : & [AccountInfo])-> 결과 <()> {
        let alice_pubkey = accounts [1] .key;
        let 명령 = token_instruction :: pay (& alice_pubkey);
        invoke (& 지시, 계정) ?;

        발사 미사일 (계정) ?;
    }
```

`invoke ()`는 Solana의 런타임에 내장되어 있으며 명령어의`program_id` 필드를 통해 주어진 명령어를`token` 프로그램으로 라우팅하는 역할을합니다.

'invoke'를 사용하려면 호출자가 호출되는 명령에 필요한 모든 계정을 전달해야합니다.  이는 실행 가능한 계정 (명령의 프로그램 ID와 일치하는 계정)과 명령 프로세서에 전달 된 계정을 의미합니다.

`pay ()`를 호출하기 전에 런타임에서`acme`가`token`이 소유 한 계정을 수정하지 않았는지 확인해야합니다. 'acme'가 'invoke'를 호출 할 때 계정의 현재 상태와 'acme'명령어 시작시 계정의 초기 상태에 런타임 정책을 적용하여이를 수행합니다. `pay ()`가 완료된 후 런타임은 런타임 정책을 다시 적용하여`token`이`acme`가 소유 한 계정을 수정하지 않았 음을 다시 확인해야하지만 이번에는`token` 프로그램 ID를 사용합니다. 마지막으로`pay_and_launch_missiles ()`가 완료된 후 런타임은 일반적으로하되 업데이트 된 모든`pre_ *`변수를 사용하여 런타임 정책을 한 번 더 적용해야합니다. `pay ()`까지`pay_and_launch_missiles ()`를 실행하면 잘못된 계정이 변경되지 않았고,`pay ()`는 잘못된 변경을하지 않았고,`pay ()`에서`pay_and_launch_missiles ()`가 반환 될 때까지 실행하면 잘못된 변경이 발생하지 않습니다. 그러면 런타임은 전체적으로 유효하지 않은 계정 변경을하지 않았으므로`pay_and_launch_missiles ()`를 전 이적으로 가정 할 수 있으므로 이러한 모든 계정 수정을 커밋 할 수 있습니다.

### 권한필요한 명령

이런타임은 호출자 프로그램에 부여 된 권한을 사용하여 호출 수신자에게 확장 할 수있는 권한을 결정합니다. 이 컨텍스트에서 권한은 서명자 및 쓰기 가능한 계정을 나타냅니다. 예를 들어, 호출자가 처리중인 명령에 서명자 또는 쓰기 가능한 계정이 포함 된 경우 호출자는 해당 서명자 및 / 또는 쓰기 가능한 계정도 포함하는 명령을 호출 할 수 있습니다.

이 권한 확장은 프로그램이 변경 불가능하다는 사실에 의존합니다. 'acme'프로그램의 경우 런타임은 트랜잭션의 서명을 '토큰'명령어의 서명으로 안전하게 처리 할 수 ​​있습니다. 런타임에서`token` 명령어가`alice_pubkey`를 참조하는 것을 확인하면`acme` 명령어에서 키를 조회하여 해당 키가 서명 된 계정에 해당하는지 확인합니다. 이 경우 '토큰'프로그램이 Alice의 계정을 수정할 수 있도록 권한을 부여합니다.

### 서명 된 계정

프로그램 파생 주소를 사용하면 \[프로그램 간 호출\] (# 크로스 프로그램 호출)시 프로그래밍 방식으로 생성 된 서명을 사용할 수 있습니다.

프로그램 프로그램은 \[프로그램 파생 주소\] (# program-derived-addresses)를 사용하여 원래 트랜잭션에 서명되지 않은 서명 된 계정이 포함 된 명령을 발행 할 수 있습니다.

```rust,ignore
        invoke_signed (
            & instruction,
            accounts,
            & [& [ "첫 번째 주소 시드"],
              & [ "두 번째 주소 첫 번째 시드", "두 번째 주소 두 번째 시드"]],
        ) ?;
```

### 호출 깊이

프로그램에서 파생 된 주소로 계정에 서명하려면 프로그램이`invoke_signed ()`를 사용할 수 있습니다.

### 재진입

재진입은 현재 고정 된 깊이로 제한되는 직접 자체 재귀로 제한됩니다. 이 제한은 프로그램이 나중에 다시 호출 될 수 있다는 것을 알지 못해도 중간 상태에서 다른 프로그램을 호출 할 수있는 상황을 방지합니다. 직접 재귀는 프로그램이 콜백되는 시점에서 상태를 완전히 제어 할 수 있도록합니다.

## 예제

Program derived addresses allow programmaticly generated signature to be used when [calling between programs](#cross-program-invocations).

프로그램 파생 주소를 사용하면 프로그램에 계정에 대한 권한이 부여되고 나중에 해당 권한을 다른 권한으로 이전 할 수 있습니다. 프로그램이 권한을 부여하는 트랜잭션에서 서명자 역할을 할 수 있기 때문에 가능합니다.

예를 들어, 두 명의 사용자가 Solana에서 게임의 결과에 대해 베팅을하려는 경우, 각각의 베팅 자산을 합의를 이행 할 중개자에게 양도해야합니다. 현재 중개 프로그램이 자산을 우승자에게 양도 할 수 없기 때문에 Solana에서 프로그램으로이 중개자를 구현할 수있는 방법이 없습니다.

This capability is necessary for many DeFi applications since they require assets to be transferred to an escrow agent until some event occurs that determines the new owner.

- Decentralized Exchanges that transfer assets between matching bid and ask orders.

- -자산을 승자에게 양도하는 경매.

- -상품을 수집하고 우승자에게 재배포하는 게임 또는 예측 시장.

이 기능은 새 소유자를 결정하는 이벤트가 발생할 때까지 자산을 에스크로 에이전트로 이전해야하기 때문에 많은 DeFi 애플리케이션에 필요합니다.

1. 프로그램이 프로그램 주소라고하는 특정 주소를 제어하도록 허용하여 외부 사용자가 해당 주소에 대한 서명을 사용하여 유효한 트랜잭션을 생성 할 수 없도록합니다.

2. 프로그램이 \[Cross-Program Invocations\] (# cross-program-invocations)를 통해 호출 된 명령어에있는 프로그램 주소에 대해 프로그래밍 방식으로 서명하도록 허용합니다.

-일치하는 입찰과 요청 주문간에 자산을 전송하는 분산형 거래소.

### 프로그램 주소에 대한 개인 키

프로그램 주소는 ed25519 곡선에 있지 않으므로 연결된 유효한 개인 키가 없으므로 서명을 생성 할 수 없습니다.  자체 개인 키는 없지만 프로그램에서 서명자로 프로그램 주소를 포함하는 명령을 발행하는 데 사용할 수 있습니다.

### 해시 기반 생성 프로그램 주소

프로그램 주소는 256 비트 사전 이미지 방지 해시 함수를 사용하여 시드 및 프로그램 ID 모음에서 결정적으로 파생됩니다.  프로그램 주소는 관련 개인 키가 없는지 확인하기 위해 ed25519 곡선에 있지 않아야합니다. 생성 중에 주소가 곡선에있는 것으로 확인되면 오류가 반환됩니다.  주어진 시드 및 프로그램 ID 모음에 대해 약 50/50 변경이 발생합니다.  이것이 발생하면 다른 시드 세트 또는 시드 범프 (추가 8 비트 시드)를 사용하여 곡선에서 유효한 프로그램 주소를 찾을 수 있습니다.

프로그램 파생 주소 :

두 가지 조건이 주어지면 사용자는 온 체인 자산의 권한을 프로그램 주소로 안전하게 이전하거나 할당 할 수 있으며 프로그램은 재량에 따라 해당 권한을 다른 곳에서 할당 할 수 있습니다.

```rust,ignore
, ignore
pub fn create_address_with_seed (
    .```rustbase : & Pubkey,
    seed : & str,
    program_id : & Pubkey,
)-&#062; Result &#060;Pubkey, SystemError&#062; {
    if seed.len ()&#062; MAX_ADDRESS_SEED_LEN {
        return Err (SystemError :: MaxSeedLengthExceeded);
    }
```

프로그램은 임의의 수의 주소를 결정적으로 파생 할 수 있습니다. 씨앗을 사용하여. 이러한 시드는 주소가 사용되는 방식을 상징적으로 식별 할 수 있습니다.

From `Pubkey`::

```rust,ignore
From`Pubkey` ::

```rust, ignore
/// 파생 프로그램 주소 생성
/// * 시드, 키를 파생하는 데 사용되는 기호 키워드
/// * program_id,대해 주소가 파생되는 프로그램
pub에fn create_program_address (
    seed : & [& [u8]],
    program_id : & Pubkey,
)-> Result <Pubkey, PubkeyError>
```

### 프로그램 주소 사용

프로그램의 결정적 프로그램 주소는`system_instruction :: create_address_with_seed`로 구현 된`SystemInstruction :: CreateAccountWithSeed`로 생성 된 계정과 유사한 파생 경로를 따릅니다.

```rust,ignore
// 에스크로 키를 결정적으로 유도
let escrow_pubkey = create_program_address (& [& [ "escrow"]], & escrow_program_id);

// 해당 키를 사용하여 전송 메시지 구성
let message = Message :: new (vec! [
    token_instruction :: transfer (& alice_pubkey, & escrow_pubkey, 1),
]);

// 1 개의 토큰을 에스크로로 전송하는 메시지를 처리합니다
클라이언트.send_and_confirm_message (& [& alice_keypair], & message);
```

프로그램은 동일한 기능을 사용하여 동일한 주소를 생성 할 수 있습니다. 아래 함수에서 프로그램은 마치 트랜잭션에 서명 할 개인 키가있는 것처럼 프로그램 주소에서 'token_instruction :: transfer'를 발행합니다.

```rust,ignore
fn transfer_one_token_from_escrow (
    program_id : & Pubkey,
    keyed_accounts : & [KeyedAccount]
)-> Result <()> {

    // 사용자가 목적지 제공
    let alice_pubkey = keyed_accounts [1] .unsigned_key ();

    // 에스크로 pubkey를 결정적으로 도출합니다.
    let escrow_pubkey = create_program_address (& [& [ "escrow"]], program_id);

    // 전송 명령 생성
    let instruction = token_instruction :: transfer (& escrow_pubkey, & alice_pubkey, 1);

    // 런타임은 현재에서 결정적으로 // 키를 파생합니다
    실행중인 프로그램 ID와 제공된 키워드.
    // 파생 주소가 명령에 서명 된 것으로 표시된 키와 일치하면
    // 해당 키는 서명 된 것으로 허용됩니다.
    invoke_signed (& instruction, & [& [ "escrow"]])?
}
```

### 서명자가 필요한 명령어

`create_program_address`로 생성 된 주소는 다른 공개 키와 구별 할 수 없습니다. 런타임에서 주소가 프로그램에 속하는지 확인하는 유일한 방법은 프로그램이 주소를 생성하는 데 사용되는 시드를 제공하는 것입니다.

}

## Examples

클라이언트는`create_program_address` 함수를 사용하여 대상 주소를 생성 할 수 있습니다.
