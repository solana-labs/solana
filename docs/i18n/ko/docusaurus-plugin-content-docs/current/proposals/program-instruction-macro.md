# 프로그램 명령 매크로

## 문제

현재 온 체인 트랜잭션을 검사하려면 명령을 구문 분석하기 위해 클라이언트 측 언어 별 디코딩 라이브러리에 의존해야합니다.  rpc 메서드가 디코딩 된 명령어 세부 정보를 반환 할 수 있다면 이러한 사용자 지정 솔루션은 필요하지 않습니다.

프로그램의 명령어 열거 형을 사용하여 명령어 데이터를 역 직렬화 할 수 있지만 계정 키 목록을 사람이 읽을 수있는 식별자로 디코딩하려면 수동 구문 분석이 필요합니다. 현재 Instruction 열거 형에는 해당 계정 정보가 있지만 변형 문서에만 있습니다.

마찬가지로 열거 형의 거의 모든 정보를 복제하는 명령어 생성자 함수가 있지만 계정 참조 목록이 코드 주석에 있기 때문에 열거 형 정의에서 해당 생성자를 생성 할 수 없습니다.

또한 일관성을 보장하는 메커니즘이 없기 때문에 지침 문서는 구현마다 다를 수 있습니다.

## 제안 된 해법

생성자를 생성 할 수 있도록 코드 주석에서 속성으로 데이터를 이동하고 열거 형 정의의 모든 문서를 포함합니다.

다음은 새 계정 형식을 사용하는 Instruction 열거 형의 예입니다.

```rust,ignore
pub enum TestInstruction {
    /// 램프 포트 전송
    전송 {
        /// 자금 계정
        자금 _ 계정 : u8
```

문서와 함께 생성 된 TestInstruction의 예 :
```rust,ignore
pub enum TestInstruction {
    /// 램프 포트 전송
    ///
    /// *이 지침에서 예상되는 계정 :
    /// 0.`[WRITABLE, SIGNER]`자금 계정
    /// 1.`[WRITABLE]`수신자 계정
    전송 {
        램프 포트 : u64,
    },

    /// M / N 필수 서명 제공
    ///
    /// *이 지침에서 예상되는 계정 :
    /// 0.`[WRITABLE]`데이터 계정
    /// * (다중)`[SIGNER]`서명자
    Multisig,

    /// 저장된 nonce를 사용하여 후속 작업으로 바꿉니다. `[WRITABLE, SIGNER]` Funding account
    ///   1. `[WRITABLE]` Recipient account
    Transfer {
        lamports: u64,
    },

    /// Provide M of N required signatures
    ///
    /// * Accounts expected by this instruction:
    ///   0. `[WRITABLE]` Data account
    ///   * (Multiple) `[SIGNER]` Signers
    Multisig,

    /// Consumes a stored nonce, replacing it with a successor
    ///
    /// * Accounts expected by this instruction:
    ///   0. `[WRITABLE, SIGNER]` Nonce account
    ///   1. `[]` RecentBlockhashes sysvar
    ///   2. (선택 사항)`[SIGNER]`Nonce 권한
    AdvanceNonceAccount,
}
```

생성 된 생성자 :
```rust,ignore
/// 램프 포트 전송
///
/// *`from_account`-`[WRITABLE, SIGNER]`자금 계정
/// *`to_account`-`[WRITABLE]`수신자 계정
pub fn transfer (from_account : Pubkey, to_account : Pubkey, lamports : u64)-> Instruction {
    let account_metas = vec! [
        AccountMeta :: new (from_pubkey, true),
        AccountMeta :: new (to_pubkey, false),
    ];
    명령 :: new (
        test_program :: id (),
        & SystemInstruction :: Transfer {lamports},
        account_metas,
    )
}

/// M / N 필수 서명 제공
///
/// *`data_account`-`[WRITABLE]`데이터 계정
/// *`signers`-(다중)`[SIGNER]`서명자
pub fn multisig (data_account : Pubkey, signers : & [Pubkey])-> Instruction {
    let mut account_metas = vec! [
        AccountMeta :: new (nonce_pubkey, false),
    ];
    for pubkey in signers.iter () {
        account_metas.push (AccountMeta :: new_readonly (pubkey, true));
    }

    명령 :: new (
        test_program :: id (),
        & TestInstruction :: Multisig,
        account_metas,
    )
}

/// 저장된 nonce를 사용하여 후속 작업으로 바꿉니다. ///
/// * nonce_account-`[WRITABLE, SIGNER]`Nonce 계정
/// * recent_blockhashes_sysvar-`[]`RecentBlockhashes sysvar
/// * nonce_authority-(선택 사항)`[SIGNER]`Nonce 권한
pub fn advance_nonce_account (
    nonce_account : Pubkey,
    recent_blockhashes_sysvar : Pubkey,
    nonce_authority : Option <Pubkey>,
)-> 명령어 {
    let mut account_metas = vec! [
        AccountMeta :: new (nonce_account, false),
        AccountMeta :: new_readonly (recent_blockhashes_sysvar, false),
    ];
    if let Some (pubkey) = authorized_pubkey {
        account_metas.push (AccountMeta :: new_readonly * nonce_authority, true));
    }
    명령 :: new (
        test_program :: id (),
        & TestInstruction :: AdvanceNonceAccount,
        account_metas,
    )
}

```

생성 된 TestInstructionVerbose 열거 형 :

```rust,ignore
/// 수신자 계정
    수신자 _ 계정 : u8

    램프 포트 : u64,
},

/// M / N 필수 서명 제공

```

## Considerations

1. **Named fields** - Since the resulting Verbose enum constructs variants with named fields, any unnamed fields in the original Instruction variant will need to have names generated. As such, it would be considerably more straightforward if all Instruction enum fields are converted to named types, instead of unnamed tuples. This seems worth doing anyway, adding more precision to the variants and enabling real documentation (so developers don't have to do [this](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140) This will cause a little churn in our current code base, but not a lot.
2. **Variable account lists** - This approach offers a couple options for variable account lists. First, optional accounts may be added and tagged with the `optional` keyword. However, currently only one optional account is supported per instruction. Additional data will need to be added to the instruction to support multiples, enabling identification of which accounts are present when some but not all are included. Second, accounts that share the same features may be added as a set, tagged with the `multiple` keyword. Like optional accounts, only one multiple account set is supported per instruction (and optional and multiple may not coexist). More complex instructions that cannot be accommodated by `optional` or `multiple`, requiring logic to figure out account order/representation, should probably be made into separate instructions.
