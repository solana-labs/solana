# Macro hướng dẫn chương trình

## Sự cố

Hiện tại, việc yêu cầu kiểm tra giao dịch trên chuỗi tùy thuộc vào thư viện giải mã ngôn ngữ cụ thể phía máy-khách để phân tích cú pháp hướng dẫn. Nếu các phương thức rpc có thể trả về các chi tiết hướng dẫn đã được giải mã, thì các giải pháp tùy chỉnh này sẽ không cần thiết.

Chúng ta có thể giải mã dữ liệu hướng dẫn bằng cách sử dụng enum Hướng dẫn của chương trình, nhưng việc giải mã danh sách khóa tài khoản thành số nhận dạng có thể đọc được của con người yêu cầu phân tích cú pháp thủ công. Các enum Hướng dẫn hiện tại của chúng tôi có thông tin tài khoản đó, nhưng chỉ trong các tài liệu biến thể.

Tương tự, chúng ta có các hàm tạo lệnh sao chép gần như tất cả thông tin trong enum, nhưng chúng ta không thể tạo hàm tạo đó từ định nghĩa enum vì danh sách các tham chiếu tài khoản nằm trong các chú thích code.

Ngoài ra, tài liệu hướng dẫn có thể khác nhau giữa các lần triển khai, vì không có cơ chế nào để đảm bảo tính nhất quán.

## Giải pháp đề xuất

Di chuyển dữ liệu từ các chú thích code sang các thuộc tính để có thể tạo ra các hàm tạo và bao gồm tất cả các tài liệu từ định nghĩa enum.

Dưới đây là một ví dụ về một enum Hướng dẫn sử dụng định dạng tài khoản mới:

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

Một ví dụ về TestInstruction được tạo bằng các tài liệu:

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

Các hàm tạo đã tạo:

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

Tạo TestInstructionVerbose enum:

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

## Cân nhắc

1. **Các trường được đặt tên** - Vì Verbose enum kết quả tạo ra các biến thể với các trường được đặt tên, bất kỳ trường nào chưa được đặt tên trong biến thể Hướng dẫn ban đầu sẽ cần phải được tạo tên. Như vậy, sẽ đơn giản hơn đáng kể nếu tất cả các trường enum của Lệnh được chuyển đổi thành các kiểu được đặt tên, thay vì các bộ giá trị không được đặt tên. Dù sao thì điều này cũng đáng làm, tăng thêm độ chính xác cho các biến thể và kích hoạt tài liệu thực (vì vậy các nhà phát triển không phải làm [điều này](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140) Điều này sẽ gây ra một chút xáo trộn trong cơ sở mã hiện tại của chúng tôi, nhưng không nhiều.
2. **Danh sách tài khoản biến đổi** - Cách tiếp cận này cung cấp một số tùy chọn cho danh sách tài khoản biến đổi. Đầu tiên, các tài khoản tùy chọn có thể được thêm vào và gắn thẻ với từ khóa `optional`. Tuy nhiên, hiện chỉ có một tài khoản tùy chọn được hỗ trợ cho mỗi hướng dẫn. Dữ liệu bổ sung sẽ cần được thêm vào hướng dẫn để hỗ trợ bội số, cho phép xác định tài khoản nào hiện diện khi một số nhưng không phải tất cả được bao gồm. Thứ hai, các tài khoản có cùng tính năng có thể được thêm vào dưới dạng một tập hợp, được gắn thẻ bằng từ khóa `multiple`. Giống như các tài khoản tùy chọn, chỉ một tập hợp nhiều tài khoản được hỗ trợ cho mỗi hướng dẫn (và tùy chọn và nhiều tài khoản có thể không cùng tồn tại). Các hướng dẫn phức tạp hơn không thể được đáp ứng bởi `optional` hoặc `multiple`, yêu cầu logic để tìm ra thứ tự/trình bày tài khoản, có lẽ nên được thực hiện thành các hướng dẫn riêng biệt.
