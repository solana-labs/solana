# 程序指令宏

## 面临的问题

当前，检查链上交易需要依赖于客户端的、特定语言的解码库来解析指令。  如果 rpc 方法可以返回解码后的指令详细信息，那么这些自定义解决方案就没有必要了。

我们可以使用程序的指令枚举来反序列化指令数据，但是将帐户密钥列表解码为人类可读的标识符需要手动解析。 我们当前的说明枚举具有该帐户信息，但仅针对变体文档。

同样，我们拥有指令构造函数，该函数几乎复制了枚举中的所有信息，但是由于帐户引用列表在代码注释中，因此无法从枚举定义中生成该构造函数。

另外，由于没有确保一致性的机制，说明文档在不同的实现方式中可能会有所不同。

## 拟定的解决方案

将数据从代码注释移到属性，以便可以生成构造函数，并包括枚举定义中的所有文档。

这是使用新帐户格式的指令枚举示例：

```rust,ignore
#[instructions(test_program::id())]
pub enum TestInstruction {
    /// 转移 lamports
    #[accounts(
        from_account(SIGNER, WRITABLE, desc = "Funding account"),
        to_account(WRITABLE, desc = "Recipient account"),
    )]
    Transfer {
        lamports: u64,
    },

    /// 提供 N 个所需签名中的 M
    #[accounts(
        data_account(WRITABLE, desc = "Data account"),
        signers(SIGNER, multiple, desc = "Signer"),
    )]
    Multisig,

    /// 消耗一个存储的nonce，用一个继承来代替
    #[accounts(
        nonce_account(SIGNER, WRITABLE, desc = "Nonce account"),
        recent_blockhashes_sysvar(desc = "RecentBlockhashes sysvar"),
        nonce_authority(SIGNER, optional, desc = "Nonce authority"),
    )]
    AdvanceNonceAccount,
}
```

用文档生成的 TestInstruction 示例：
```rust,ignore
pub enum TestInstruction {
    /// 转移 lamports
    ///
    /// * 此操作需要的账户：
    ///   0。 `[WRITABLE, SIGNER]` Funding account
    ///   1. `[WRITABLE]` Recipient account
    Transfer {
        lamports: u64,
    },

    /// 提供 N 个所需签名中的 M
    ///
    /// * 此操作需要的账户：
    ///   0。 `[WRITABLE]` Data account
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

生成的构造器：
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

生成的 TestInstructionVerbose 枚举：

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

## 考虑因素

1. **命名字段（Named fields）** - 由于生成的Verbose枚举使用命名字段构造变量，因此原始指令变量中的所有未命名字段都需要生成名称。 这样，如果将所有Instruction枚举字段都转换为命名类型而不是未命名元组，就会更加直接。 无论如何，这似乎都是值得的，因为它可以为变体增加更多的精度并启用真实文档(因此开发人员不必[这样做](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140)。它会在我们当前的代码库中造成一点混乱，但不会太严重。
2. **可变帐户列表（Variable account lists）** - 这种方法为可变帐户列表提供了两个选项。 首先是添加可选帐户，并使用`optional`关键字进行标记。 但是，当前每条指令仅支持一个可选帐户。 需要在指令中添加其他数据以支持乘数，从而能够在包含一些但不是全部时识别存在的帐户。 其次，可以将具有相同功能的帐户作为一个集合添加，并以关键字`multiple`标记。 与可选帐户一样，每条指令仅支持一个多帐户集(并且可选和多个帐户可能不共存)。 可能需要将逻辑弄清楚帐户顺序/表示形式的，不能由`可选`或`多个`容纳的更复杂的指令，可能应该做成单独的指令。
