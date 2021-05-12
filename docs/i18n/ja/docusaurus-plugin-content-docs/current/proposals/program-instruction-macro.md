# プログラム命令マクロ

## 問題

現在、オンチェーントランザクションを検査するには、クライアントサイドの言語固有のデコードライブラリを使用して命令を解析する必要があります。 "rpc メソッド"がデコードされた命令の詳細を返すことができれば、このようなカスタムソリューションは不要になります。

プログラムの"Instruction enum"を使用して命令データをデシリアライズすることはできますが、アカウントキーのリストを人間が読める識別子にデコードするには、手動での解析が必要です。 現在の"Instruction enum"はアカウント情報を持っていますが、バリアントドキュメントにしかありません。

同様に、列挙型のほぼすべての情報を複製する命令コンストラクタ関数がありますが、アカウント参照のリストがコードコメントにあるため、列挙型の定義からそのコンストラクタを生成することはできません。

また、インストラクションのドキュメントは、一貫性を確保するメカニズムがないため、実装間で異なる可能性があります。

## 提案された解決策

データをコードコメントから属性に移動し、コンストラクタを生成できるようにし、列挙型定義からすべてのドキュメントを含めるようにします。

以下は、新しいアカウント形式を使用した Instruction enum 命令列挙の例です。

```rust,ignore
#[instructions(test_program::id())]
pub enum TestInstruction {
    /// Transfer lamports
    #[accounts(
        from_account(SIGNER, WRITABLE, desc = "Funding account"),
        to_account(WRITABLE, desc = "Recipient account"),
    )]
    Transfer {
        lamports: u64,
    },

    /// Provide M of N required signatures
    #[accounts(
        data_account(WRITABLE, desc = "Data account"),
        signers(SIGNER, multiple, desc = "Signer"),
    )]
    Multisig,

    /// Consumes a stored nonce, replacing it with a successor
    #[accounts(
        nonce_account(SIGNER, WRITABLE, desc = "Nonce account"),
        recent_blockhashes_sysvar(desc = "RecentBlockhashes sysvar"),
        nonce_authority(SIGNER, optional, desc = "Nonce authority"),
    )]
    AdvanceNonceAccount,
}
```

生成された"TestInstruction"の docs 付きの例です。

```rust,ignore
pub enum TestInstruction {
    /// Transfer lamports
    ///
    /// * Accounts expected by this instruction:
    ///   0. `[WRITABLE, SIGNER]` Funding account
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
    ///   2. (Optional) `[SIGNER]` Nonce authority
    AdvanceNonceAccount,
}
```

生成されたコンストラクタ:

```rust,ignore
/// Transfer lamports
///
/// * `from_account` - `[WRITABLE, SIGNER]` Funding account
/// * `to_account` - `[WRITABLE]` Recipient account
pub fn transfer(from_account: Pubkey, to_account: Pubkey, lamports: u64) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(from_pubkey, true),
        AccountMeta::new(to_pubkey, false),
    ];
    Instruction::new(
        test_program::id(),
        &SystemInstruction::Transfer { lamports },
        account_metas,
    )
}

/// Provide M of N required signatures
///
/// * `data_account` - `[WRITABLE]` Data account
/// * `signers` - (Multiple) `[SIGNER]` Signers
pub fn multisig(data_account: Pubkey, signers: &[Pubkey]) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(nonce_pubkey, false),
    ];
    for pubkey in signers.iter() {
        account_metas.push(AccountMeta::new_readonly(pubkey, true));
    }

    Instruction::new(
        test_program::id(),
        &TestInstruction::Multisig,
        account_metas,
    )
}

/// Consumes a stored nonce, replacing it with a successor
///
/// * nonce_account - `[WRITABLE, SIGNER]` Nonce account
/// * recent_blockhashes_sysvar - `[]` RecentBlockhashes sysvar
/// * nonce_authority - (Optional) `[SIGNER]` Nonce authority
pub fn advance_nonce_account(
    nonce_account: Pubkey,
    recent_blockhashes_sysvar: Pubkey,
    nonce_authority: Option<Pubkey>,
) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(nonce_account, false),
        AccountMeta::new_readonly(recent_blockhashes_sysvar, false),
    ];
    if let Some(pubkey) = authorized_pubkey {
        account_metas.push(AccountMeta::new_readonly*nonce_authority, true));
    }
    Instruction::new(
        test_program::id(),
        &TestInstruction::AdvanceNonceAccount,
        account_metas,
    )
}

```

生成された TestInstructionVerbose enum。

```rust,ignore
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TestInstruction {
    /// Transfer lamports
    Transfer {
        /// Funding account
        funding_account: u8

        /// Recipient account
        recipient_account: u8

        lamports: u64,
    },

    /// Provide M of N required signatures
    Multisig {
        data_account: u8,
        signers: Vec<u8>,
    },

    /// Consumes a stored nonce, replacing it with a successor
    AdvanceNonceAccount {
        nonce_account: u8,
        recent_blockhashes_sysvar: u8,
        nonce_authority: Option<u8>,
    }
}

impl TestInstructionVerbose {
    pub fn from_instruction(instruction: TestInstruction, account_keys: Vec<u8>) -> Self {
        match instruction {
            TestInstruction::Transfer { lamports } => TestInstructionVerbose::Transfer {
                funding_account: account_keys[0],
                recipient_account: account_keys[1],
                lamports,
            }
            TestInstruction::Multisig => TestInstructionVerbose::Multisig {
                data_account: account_keys[0],
                signers: account_keys[1..],
            }
            TestInstruction::AdvanceNonceAccount => TestInstructionVerbose::AdvanceNonceAccount {
                nonce_account: account_keys[0],
                recent_blockhashes_sysvar: account_keys[1],
                nonce_authority: &account_keys.get(2),
            }
        }
    }
}

```

## 考慮事項

1. **Named fields** - 結果的に"Verbose enum"は名前のついたフィールドを持つバリアントを構築するので、オリジナルの"Instruction バリアント"内の名前のないフィールドには名前を生成する必要があります。 このように、すべての"Instruction enum"のフィールドが無名のタプルではなく、名前付きのタイプに変換されると、かなり簡単になります。 これはいずれにしてもやる価値があると思います。バリアントの精度を高め、本当の意味でのドキュメンテーションを可能にします(そうすれば開発者は[これ](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140)をする必要がありません。これは現在のコードベースを少し変えることになりますが、それほど多くはありません
2. **Variable account lists** - この方法では、可変アカウントリストにいくつかのオプションがあります。 まず、オプションのアカウントを追加し、`optional`キーワードでタグ付けすることができます。 しかし、現時点では 1 つの命令に対して 1 つのオプションアカウントしかサポートされていません。 複数のアカウントをサポートするためには、命令に追加データを追加する必要があります。これにより、一部のアカウントが含まれていても、すべてのアカウントが含まれていない場合に、どのアカウントが存在するかを識別することができます。 2 つ目は、同じ機能を持つアカウントをセットで追加し、`multiple`キーワードでタグ付けすることです。 オプションのアカウントと同様に、1 つの命令でサポートされる複数アカウントセットは 1 つだけです(オプションと multiple は共存できません)。 `optional`や`multiple`では対応できない複雑な命令、つまりアカウントの順番や表現を考えるロジックが必要な命令は、おそらく別の命令にすべきでしょう。
