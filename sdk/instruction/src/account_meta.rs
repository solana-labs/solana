use solana_pubkey::Pubkey;

/// Describes a single account read or written by a program during instruction
/// execution.
///
/// When constructing an [`Instruction`], a list of all accounts that may be
/// read or written during the execution of that instruction must be supplied.
/// Any account that may be mutated by the program during execution, either its
/// data or metadata such as held lamports, must be writable.
///
/// Note that because the Solana runtime schedules parallel transaction
/// execution around which accounts are writable, care should be taken that only
/// accounts which actually may be mutated are specified as writable. As the
/// default [`AccountMeta::new`] constructor creates writable accounts, this is
/// a minor hazard: use [`AccountMeta::new_readonly`] to specify that an account
/// is not writable.
///
/// [`Instruction`]: crate::Instruction
#[repr(C)]
#[cfg_attr(
    feature = "serde",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct AccountMeta {
    /// An account's public key.
    pub pubkey: Pubkey,
    /// True if an `Instruction` requires a `Transaction` signature matching `pubkey`.
    pub is_signer: bool,
    /// True if the account data or metadata may be mutated during program execution.
    pub is_writable: bool,
}

impl AccountMeta {
    /// Construct metadata for a writable account.
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_pubkey::Pubkey;
    /// # use solana_instruction::{AccountMeta, Instruction};
    /// # use borsh::{BorshSerialize, BorshDeserialize};
    /// #
    /// # #[derive(BorshSerialize, BorshDeserialize)]
    /// # #[borsh(crate = "borsh")]
    /// # pub struct MyInstruction;
    /// #
    /// # let instruction = MyInstruction;
    /// # let from = Pubkey::new_unique();
    /// # let to = Pubkey::new_unique();
    /// # let program_id = Pubkey::new_unique();
    /// let instr = Instruction::new_with_borsh(
    ///     program_id,
    ///     &instruction,
    ///     vec![
    ///         AccountMeta::new(from, true),
    ///         AccountMeta::new(to, false),
    ///     ],
    /// );
    /// ```
    pub fn new(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: true,
        }
    }

    /// Construct metadata for a read-only account.
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_pubkey::Pubkey;
    /// # use solana_instruction::{AccountMeta, Instruction};
    /// # use borsh::{BorshSerialize, BorshDeserialize};
    /// #
    /// # #[derive(BorshSerialize, BorshDeserialize)]
    /// # #[borsh(crate = "borsh")]
    /// # pub struct MyInstruction;
    /// #
    /// # let instruction = MyInstruction;
    /// # let from = Pubkey::new_unique();
    /// # let to = Pubkey::new_unique();
    /// # let from_account_storage = Pubkey::new_unique();
    /// # let program_id = Pubkey::new_unique();
    /// let instr = Instruction::new_with_borsh(
    ///     program_id,
    ///     &instruction,
    ///     vec![
    ///         AccountMeta::new(from, true),
    ///         AccountMeta::new(to, false),
    ///         AccountMeta::new_readonly(from_account_storage, false),
    ///     ],
    /// );
    /// ```
    pub fn new_readonly(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: false,
        }
    }
}
