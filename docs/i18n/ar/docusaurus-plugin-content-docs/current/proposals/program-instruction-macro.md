# برنامج التعليمات الكلي (Program Instruction Macro)

## المُشكل

في الوقت الحالي، يتطلب فحص مُعاملة على الشبكة (On-Chain) إعتمادًا على مكتبة فك تشفير خاصة باللغات من جانب العميل لتحليل التعليمات. إذا تمكنت طرق الـ rpc من إرجاع تفاصيل التعليمات التي تم فك ترميزها، فلن تكون هذه الحلول المُخَصَّصة ضرورية.

يُمكننا إلغاء تسلسل بيانات التعليمات بإستخدام تعداد تعليمات البرنامج، لكن فك تشفير قائمة مفتاح الحساب (account-key) إلى مُعرفات مُمكن للبشر قراءتها يتطلب تحليلًا يدويًا. تحتوي تعدادات تعليماتنا الحالية على معلومات الحساب هذه، ولكن فقط في مُستندات مُختلفة.

بالمثل، لدينا وظائف مُنشئ التعليمات التي تُكرر جميع المعلومات الموجودة في التعداد تقريبًا، ولكن لا يمكننا إنشاء هذا المُنشئ من تعريف التعداد لأن قائمة مراجع الحساب موجودة في تعليقات التعليمات البرمجية.

يُمكن أيضًا أن تختلف مُستندات التعليمات بين التطبيقات، حيث لا توجد آلية لضمان الإتساق.

## الحل المُقترح (Proposed Solution)

قُم بنقل البيانات من تعليقات التعليمات البرمجية (code comments) إلى السِّمات (attributes)، بحيث يُمكن إنشاء المُنشئات (constructors)، وتضمين جميع الوثائق من تعريف التعداد.

فيما يلي مثال على تعداد تعليمي بإستخدام تنسيق الحسابات الجديد:

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

مثال لتعليمات الإختبار التي تم إنشاؤها بإستخدام المُستندات:

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

المُنشئات المُولَّدة (Generated constructors):

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
    Instruction::new_with_bincode(
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

    Instruction::new_with_bincode(
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
    Instruction::new_with_bincode(
        test_program::id(),
        &TestInstruction::AdvanceNonceAccount,
        account_metas,
    )
}

```

إرشادات الإختبار التعدادي المطوّل (Generated TestInstructionVerbose enum):

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

## الإعتبارات (Considerations)

1. الحقول المُسَمَّاة **Named fields** - نظرًا لأن تعداد Verbose الناتج يبني مُتغيرات مع الحقول المُسماة، فإن أي حقول غير مُسماة في مُتغير (variant) التعليمات الأصلي ستحتاج إلى إنشاء أسماء. على هذا النحو، سيكون الأمر أكثر وضوحًا إذا تم تحويل جميع حقول تعداد التعليمات إلى أنواع مُسماة، بدلاً من المجموعات غير المُسماة. يبدو أن هذا يستحق القيام به على أي حال، إضافة المزيد من الدقة إلى المُتغيرات (variants) وتمكين التوثيق الحقيقي (حتى لا يضطر المطورون إلى القيام بـ [this](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140) وهذا سيُؤدي إلى حدوث القليل من التغيير في قاعدة الشفرة الحالية، ولكن ليس كثيرًا.
2. قوائم الحسابات المُتَغَيِّرَة **Variable account lists** - يُوفر هذا الأسلوب عِدَّة خيارات لقوائم الحسابات المُتَغَيِّرَة. أولاً، يمكن إضافة حسابات إختيارية وتمييزها بإستخدام كلمة رئيسية `optional`. مع ذلك، يتم حاليًا دعم حساب إختياري واحد فقط لكل تعليمة. يجب إضافة بيانات إضافية إلى التعليمة (instruction) لدعم المُضاعفات، مما يُتيح تحديد الحسابات الموجودة عند تضمين بعضها وليس كلها. ثانيًا، يُمكن إضافة الحسابات التي تشترك في نفس الميزات كمجموعة، يتم تمييزها بالكلمة الأساسية `multiple`. مثل الحسابات الإختيارية، يتم دعم مجموعة حسابات مُتعددة واحدة فقط لكل تعليمة (instruction) (وقد لا تتواجد مجموعة إختيارية ومُتعددة). التعليمات الأكثر تعقيدًا التي لا يُمكن إستيعابها بواسطة `optional` أو `multiple`، والتي تتطلب منطقًا لمعرفة أمر / تمثيل الحساب، يجب أن يتم تحويلها على الأرجح إلى تعليمات (instructions) مُنفصلة.
