# Инструкция по программе Макро

## Проблематика

В настоящее время проверка транзакции в цепочке требует зависимости от клиентской библиотеки декодирования, зависящей от языка, для синтаксического анализа инструкции.  Если бы методы rpc могли возвращать декодированные детали инструкций, в этих специальных решениях не было бы необходимости.

Мы можем десериализовать данные инструкций, используя перечисление инструкций программы, но декодирование списка ключей аккаунта в удобочитаемые идентификаторы требует ручного анализа. В наших текущих списках инструкций есть информация об этом аккаунте, но только в вариантах документов.

Точно так же у нас есть функции конструктора инструкций, которые дублируют почти всю информацию в перечислении, но мы не можем сгенерировать этот конструктор из определения перечисления, потому что список ссылок на аккаунты находится в комментариях к коду.

Кроме того, документы с инструкциями могут различаться в зависимости от реализации, поскольку отсутствует механизм обеспечения согласованности.

## Предлагаемое решение

Переместите данные из комментариев кода в атрибуты, чтобы можно было сгенерировать конструкторы, и включите всю документацию из определения перечисления.

Вот пример инструкции по формату новых аккаунтов:

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

Пример созданной инструкции по тестированию с документами:
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

Сгенерированные конструкторы:
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

Сгенерированное перечисление TestInstructionVerbose:

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

## Рекомендации

1. **Named fields** - поскольку результирующее подробное перечисление создает варианты с именованными полями, для любых безымянных полей в исходном варианте инструкции должны быть сгенерированы имена. Таким образом, было бы намного проще, если бы все поля перечисления инструкций были преобразованы в именованные типы вместо безымянных кортежей. В любом случае кажется, что это стоит сделать, добавив больше точности к вариантам и включив реальную документацию (так что разработчикам не нужно делать [ это ](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140). Это вызовет небольшой отток в нашей текущей базе кода, но не сильно.
2. **Variable account lists** - этот подход предлагает несколько вариантов для списков переменных аккаунтов. Во-первых, необязательные аккаунты могут быть добавлены и помечены ключевым словом ` optional `. Однако в настоящее время для каждой инструкции поддерживается только один дополнительный аккаунт. В инструкцию необходимо будет добавить дополнительные данные для поддержки кратных, позволяющих идентифицировать, какие аккаунты присутствуют, когда включены некоторые, но не все. Во-вторых, аккаунты с одинаковыми функциями могут быть добавлены в виде набора, помеченного ключевым словом ` multiple `. Как и в случае необязательных аккаунтов, для каждой инструкции поддерживается только один набор нескольких аккаунтов (а необязательные и несколько аккаунтов могут не сосуществовать). Более сложные инструкции, которые не могут быть размещены с помощью `optional` или `multiple`, требующих логики для определения порядка / представления аккаунтов, вероятно, следует оформить в отдельные инструкции.
