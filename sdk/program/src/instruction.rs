//! Types for directing the execution of Solana programs.
//!
//! Every invocation of a Solana program executes a single instruction, as
//! defined by the [`Instruction`] type. An instruction is primarily a vector of
//! bytes, the contents of which are program-specific, and not interpreted by
//! the Solana runtime. This allows flexibility in how programs behave, how they
//! are controlled by client software, and what data encodings they use.
//!
//! Besides the instruction data, every account a program may read or write
//! while executing a given instruction is also included in `Instruction`, as
//! [`AccountMeta`] values. The runtime uses this information to efficiently
//! schedule execution of transactions.

#![allow(clippy::integer_arithmetic)]

use {
    crate::{pubkey::Pubkey, sanitize::Sanitize, short_vec, wasm_bindgen},
    bincode::serialize,
    borsh::BorshSerialize,
    serde::Serialize,
    thiserror::Error,
};

/// Reasons the runtime might have rejected an instruction.
///
/// Instructions errors are included in the bank hashes and therefore are
/// included as part of the transaction results when determining consensus.
/// Because of this, members of this enum must not be removed, but new ones can
/// be added.  Also, it is crucial that meta-information if any that comes along
/// with an error be consistent across software versions.  For example, it is
/// dangerous to include error strings from 3rd party crates because they could
/// change at any time and changes to them are difficult to detect.
#[derive(
    Serialize, Deserialize, Debug, Error, PartialEq, Eq, Clone, AbiExample, AbiEnumVisitor,
)]
pub enum InstructionError {
    /// Deprecated! Use CustomError instead!
    /// The program instruction returned an error
    #[error("generic instruction error")]
    GenericError,

    /// The arguments provided to a program were invalid
    #[error("invalid program argument")]
    InvalidArgument,

    /// An instruction's data contents were invalid
    #[error("invalid instruction data")]
    InvalidInstructionData,

    /// An account's data contents was invalid
    #[error("invalid account data for instruction")]
    InvalidAccountData,

    /// An account's data was too small
    #[error("account data too small for instruction")]
    AccountDataTooSmall,

    /// An account's balance was too small to complete the instruction
    #[error("insufficient funds for instruction")]
    InsufficientFunds,

    /// The account did not have the expected program id
    #[error("incorrect program id for instruction")]
    IncorrectProgramId,

    /// A signature was required but not found
    #[error("missing required signature for instruction")]
    MissingRequiredSignature,

    /// An initialize instruction was sent to an account that has already been initialized.
    #[error("instruction requires an uninitialized account")]
    AccountAlreadyInitialized,

    /// An attempt to operate on an account that hasn't been initialized.
    #[error("instruction requires an initialized account")]
    UninitializedAccount,

    /// Program's instruction lamport balance does not equal the balance after the instruction
    #[error("sum of account balances before and after instruction do not match")]
    UnbalancedInstruction,

    /// Program illegally modified an account's program id
    #[error("instruction illegally modified the program id of an account")]
    ModifiedProgramId,

    /// Program spent the lamports of an account that doesn't belong to it
    #[error("instruction spent from the balance of an account it does not own")]
    ExternalAccountLamportSpend,

    /// Program modified the data of an account that doesn't belong to it
    #[error("instruction modified data of an account it does not own")]
    ExternalAccountDataModified,

    /// Read-only account's lamports modified
    #[error("instruction changed the balance of a read-only account")]
    ReadonlyLamportChange,

    /// Read-only account's data was modified
    #[error("instruction modified data of a read-only account")]
    ReadonlyDataModified,

    /// An account was referenced more than once in a single instruction
    // Deprecated, instructions can now contain duplicate accounts
    #[error("instruction contains duplicate accounts")]
    DuplicateAccountIndex,

    /// Executable bit on account changed, but shouldn't have
    #[error("instruction changed executable bit of an account")]
    ExecutableModified,

    /// Rent_epoch account changed, but shouldn't have
    #[error("instruction modified rent epoch of an account")]
    RentEpochModified,

    /// The instruction expected additional account keys
    #[error("insufficient account keys for instruction")]
    NotEnoughAccountKeys,

    /// Program other than the account's owner changed the size of the account data
    #[error("program other than the account's owner changed the size of the account data")]
    AccountDataSizeChanged,

    /// The instruction expected an executable account
    #[error("instruction expected an executable account")]
    AccountNotExecutable,

    /// Failed to borrow a reference to account data, already borrowed
    #[error("instruction tries to borrow reference for an account which is already borrowed")]
    AccountBorrowFailed,

    /// Account data has an outstanding reference after a program's execution
    #[error("instruction left account with an outstanding borrowed reference")]
    AccountBorrowOutstanding,

    /// The same account was multiply passed to an on-chain program's entrypoint, but the program
    /// modified them differently.  A program can only modify one instance of the account because
    /// the runtime cannot determine which changes to pick or how to merge them if both are modified
    #[error("instruction modifications of multiply-passed account differ")]
    DuplicateAccountOutOfSync,

    /// Allows on-chain programs to implement program-specific error types and see them returned
    /// by the Solana runtime. A program-specific error may be any type that is represented as
    /// or serialized to a u32 integer.
    #[error("custom program error: {0:#x}")]
    Custom(u32),

    /// The return value from the program was invalid.  Valid errors are either a defined builtin
    /// error value or a user-defined error in the lower 32 bits.
    #[error("program returned invalid error code")]
    InvalidError,

    /// Executable account's data was modified
    #[error("instruction changed executable accounts data")]
    ExecutableDataModified,

    /// Executable account's lamports modified
    #[error("instruction changed the balance of a executable account")]
    ExecutableLamportChange,

    /// Executable accounts must be rent exempt
    #[error("executable accounts must be rent exempt")]
    ExecutableAccountNotRentExempt,

    /// Unsupported program id
    #[error("Unsupported program id")]
    UnsupportedProgramId,

    /// Cross-program invocation call depth too deep
    #[error("Cross-program invocation call depth too deep")]
    CallDepth,

    /// An account required by the instruction is missing
    #[error("An account required by the instruction is missing")]
    MissingAccount,

    /// Cross-program invocation reentrancy not allowed for this instruction
    #[error("Cross-program invocation reentrancy not allowed for this instruction")]
    ReentrancyNotAllowed,

    /// Length of the seed is too long for address generation
    #[error("Length of the seed is too long for address generation")]
    MaxSeedLengthExceeded,

    /// Provided seeds do not result in a valid address
    #[error("Provided seeds do not result in a valid address")]
    InvalidSeeds,

    /// Failed to reallocate account data of this length
    #[error("Failed to reallocate account data")]
    InvalidRealloc,

    /// Computational budget exceeded
    #[error("Computational budget exceeded")]
    ComputationalBudgetExceeded,

    /// Cross-program invocation with unauthorized signer or writable account
    #[error("Cross-program invocation with unauthorized signer or writable account")]
    PrivilegeEscalation,

    /// Failed to create program execution environment
    #[error("Failed to create program execution environment")]
    ProgramEnvironmentSetupFailure,

    /// Program failed to complete
    #[error("Program failed to complete")]
    ProgramFailedToComplete,

    /// Program failed to compile
    #[error("Program failed to compile")]
    ProgramFailedToCompile,

    /// Account is immutable
    #[error("Account is immutable")]
    Immutable,

    /// Incorrect authority provided
    #[error("Incorrect authority provided")]
    IncorrectAuthority,

    /// Failed to serialize or deserialize account data
    ///
    /// Warning: This error should never be emitted by the runtime.
    ///
    /// This error includes strings from the underlying 3rd party Borsh crate
    /// which can be dangerous because the error strings could change across
    /// Borsh versions. Only programs can use this error because they are
    /// consistent across Solana software versions.
    ///
    #[error("Failed to serialize or deserialize account data: {0}")]
    BorshIoError(String),

    /// An account does not have enough lamports to be rent-exempt
    #[error("An account does not have enough lamports to be rent-exempt")]
    AccountNotRentExempt,

    /// Invalid account owner
    #[error("Invalid account owner")]
    InvalidAccountOwner,

    /// Program arithmetic overflowed
    #[error("Program arithmetic overflowed")]
    ArithmeticOverflow,

    /// Unsupported sysvar
    #[error("Unsupported sysvar")]
    UnsupportedSysvar,

    /// Illegal account owner
    #[error("Provided owner is not allowed")]
    IllegalOwner,

    /// Accounts data allocations exceeded the maximum allowed per transaction
    #[error("Accounts data allocations exceeded the maximum allowed per transaction")]
    MaxAccountsDataAllocationsExceeded,

    /// Max accounts exceeded
    #[error("Max accounts exceeded")]
    MaxAccountsExceeded,

    /// Max instruction trace length exceeded
    #[error("Max instruction trace length exceeded")]
    MaxInstructionTraceLengthExceeded,
    // Note: For any new error added here an equivalent ProgramError and its
    // conversions must also be added
}

/// A directive for a single invocation of a Solana program.
///
/// An instruction specifies which program it is calling, which accounts it may
/// read or modify, and additional data that serves as input to the program. One
/// or more instructions are included in transactions submitted by Solana
/// clients. Instructions are also used to describe [cross-program
/// invocations][cpi].
///
/// [cpi]: https://docs.solana.com/developing/programming-model/calling-between-programs
///
/// During execution, a program will receive a list of account data as one of
/// its arguments, in the same order as specified during `Instruction`
/// construction.
///
/// While Solana is agnostic to the format of the instruction data, it has
/// built-in support for serialization via [`borsh`] and [`bincode`].
///
/// [`borsh`]: https://docs.rs/borsh/latest/borsh/
/// [`bincode`]: https://docs.rs/bincode/latest/bincode/
///
/// # Specifying account metadata
///
/// When constructing an [`Instruction`], a list of all accounts that may be
/// read or written during the execution of that instruction must be supplied as
/// [`AccountMeta`] values.
///
/// Any account whose data may be mutated by the program during execution must
/// be specified as writable. During execution, writing to an account that was
/// not specified as writable will cause the transaction to fail. Writing to an
/// account that is not owned by the program will cause the transaction to fail.
///
/// Any account whose lamport balance may be mutated by the program during
/// execution must be specified as writable. During execution, mutating the
/// lamports of an account that was not specified as writable will cause the
/// transaction to fail. While _subtracting_ lamports from an account not owned
/// by the program will cause the transaction to fail, _adding_ lamports to any
/// account is allowed, as long is it is mutable.
///
/// Accounts that are not read or written by the program may still be specified
/// in an `Instruction`'s account list. These will affect scheduling of program
/// execution by the runtime, but will otherwise be ignored.
///
/// When building a transaction, the Solana runtime coalesces all accounts used
/// by all instructions in that transaction, along with accounts and permissions
/// required by the runtime, into a single account list. Some accounts and
/// account permissions required by the runtime to process a transaction are
/// _not_ required to be included in an `Instruction`s account list. These
/// include:
///
/// - The program ID &mdash; it is a separate field of `Instruction`
/// - The transaction's fee-paying account &mdash; it is added during [`Message`]
///   construction. A program may still require the fee payer as part of the
///   account list if it directly references it.
///
/// [`Message`]: crate::message::Message
///
/// Programs may require signatures from some accounts, in which case they
/// should be specified as signers during `Instruction` construction. The
/// program must still validate during execution that the account is a signer.
#[wasm_bindgen]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Instruction {
    /// Pubkey of the program that executes this instruction.
    #[wasm_bindgen(skip)]
    pub program_id: Pubkey,
    /// Metadata describing accounts that should be passed to the program.
    #[wasm_bindgen(skip)]
    pub accounts: Vec<AccountMeta>,
    /// Opaque data passed to the program for its own interpretation.
    #[wasm_bindgen(skip)]
    pub data: Vec<u8>,
}

impl Instruction {
    /// Create a new instruction from a value, encoded with [`borsh`].
    ///
    /// [`borsh`]: https://docs.rs/borsh/latest/borsh/
    ///
    /// `program_id` is the address of the program that will execute the instruction.
    /// `accounts` contains a description of all accounts that may be accessed by the program.
    ///
    /// Borsh serialization is often prefered over bincode as it has a stable
    /// [specification] and an [implementation in JavaScript][jsb], neither of
    /// which are true of bincode.
    ///
    /// [specification]: https://borsh.io/
    /// [jsb]: https://github.com/near/borsh-js
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_program::{
    /// #     pubkey::Pubkey,
    /// #     instruction::{AccountMeta, Instruction},
    /// # };
    /// # use borsh::{BorshSerialize, BorshDeserialize};
    /// #
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// pub struct MyInstruction {
    ///     pub lamports: u64,
    /// }
    ///
    /// pub fn create_instruction(
    ///     program_id: &Pubkey,
    ///     from: &Pubkey,
    ///     to: &Pubkey,
    ///     lamports: u64,
    /// ) -> Instruction {
    ///     let instr = MyInstruction { lamports };
    ///
    ///     Instruction::new_with_borsh(
    ///         *program_id,
    ///         &instr,
    ///         vec![
    ///             AccountMeta::new(*from, true),
    ///             AccountMeta::new(*to, false),
    ///         ],
    ///    )
    /// }
    /// ```
    pub fn new_with_borsh<T: BorshSerialize>(
        program_id: Pubkey,
        data: &T,
        accounts: Vec<AccountMeta>,
    ) -> Self {
        let data = data.try_to_vec().unwrap();
        Self {
            program_id,
            accounts,
            data,
        }
    }

    /// Create a new instruction from a value, encoded with [`bincode`].
    ///
    /// [`bincode`]: https://docs.rs/bincode/latest/bincode/
    ///
    /// `program_id` is the address of the program that will execute the instruction.
    /// `accounts` contains a description of all accounts that may be accessed by the program.
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_program::{
    /// #     pubkey::Pubkey,
    /// #     instruction::{AccountMeta, Instruction},
    /// # };
    /// # use serde::{Serialize, Deserialize};
    /// #
    /// #[derive(Serialize, Deserialize)]
    /// pub struct MyInstruction {
    ///     pub lamports: u64,
    /// }
    ///
    /// pub fn create_instruction(
    ///     program_id: &Pubkey,
    ///     from: &Pubkey,
    ///     to: &Pubkey,
    ///     lamports: u64,
    /// ) -> Instruction {
    ///     let instr = MyInstruction { lamports };
    ///
    ///     Instruction::new_with_bincode(
    ///         *program_id,
    ///         &instr,
    ///         vec![
    ///             AccountMeta::new(*from, true),
    ///             AccountMeta::new(*to, false),
    ///         ],
    ///    )
    /// }
    /// ```
    pub fn new_with_bincode<T: Serialize>(
        program_id: Pubkey,
        data: &T,
        accounts: Vec<AccountMeta>,
    ) -> Self {
        let data = serialize(data).unwrap();
        Self {
            program_id,
            accounts,
            data,
        }
    }

    /// Create a new instruction from a byte slice.
    ///
    /// `program_id` is the address of the program that will execute the instruction.
    /// `accounts` contains a description of all accounts that may be accessed by the program.
    ///
    /// The caller is responsible for ensuring the correct encoding of `data` as expected
    /// by the callee program.
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_program::{
    /// #     pubkey::Pubkey,
    /// #     instruction::{AccountMeta, Instruction},
    /// # };
    /// # use borsh::{BorshSerialize, BorshDeserialize};
    /// # use anyhow::Result;
    /// #
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// pub struct MyInstruction {
    ///     pub lamports: u64,
    /// }
    ///
    /// pub fn create_instruction(
    ///     program_id: &Pubkey,
    ///     from: &Pubkey,
    ///     to: &Pubkey,
    ///     lamports: u64,
    /// ) -> Result<Instruction> {
    ///     let instr = MyInstruction { lamports };
    ///
    ///     let mut instr_in_bytes: Vec<u8> = Vec::new();
    ///     instr.serialize(&mut instr_in_bytes)?;
    ///
    ///     Ok(Instruction::new_with_bytes(
    ///         *program_id,
    ///         &instr_in_bytes,
    ///         vec![
    ///             AccountMeta::new(*from, true),
    ///             AccountMeta::new(*to, false),
    ///         ],
    ///    ))
    /// }
    /// ```
    pub fn new_with_bytes(program_id: Pubkey, data: &[u8], accounts: Vec<AccountMeta>) -> Self {
        Self {
            program_id,
            accounts,
            data: data.to_vec(),
        }
    }

    #[deprecated(
        since = "1.6.0",
        note = "Please use another Instruction constructor instead, such as `Instruction::new_with_borsh`"
    )]
    pub fn new<T: Serialize>(program_id: Pubkey, data: &T, accounts: Vec<AccountMeta>) -> Self {
        Self::new_with_bincode(program_id, data, accounts)
    }
}

/// Addition that returns [`InstructionError::InsufficientFunds`] on overflow.
///
/// This is an internal utility function.
#[doc(hidden)]
pub fn checked_add(a: u64, b: u64) -> Result<u64, InstructionError> {
    a.checked_add(b).ok_or(InstructionError::InsufficientFunds)
}

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
#[repr(C)]
#[derive(Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
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
    /// # use solana_program::{
    /// #     pubkey::Pubkey,
    /// #     instruction::{AccountMeta, Instruction},
    /// # };
    /// # use borsh::{BorshSerialize, BorshDeserialize};
    /// #
    /// # #[derive(BorshSerialize, BorshDeserialize)]
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
    /// # use solana_program::{
    /// #     pubkey::Pubkey,
    /// #     instruction::{AccountMeta, Instruction},
    /// # };
    /// # use borsh::{BorshSerialize, BorshDeserialize};
    /// #
    /// # #[derive(BorshSerialize, BorshDeserialize)]
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

/// A compact encoding of an instruction.
///
/// A `CompiledInstruction` is a component of a multi-instruction [`Message`],
/// which is the core of a Solana transaction. It is created during the
/// construction of `Message`. Most users will not interact with it directly.
///
/// [`Message`]: crate::message::Message
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct CompiledInstruction {
    /// Index into the transaction keys array indicating the program account that executes this instruction.
    pub program_id_index: u8,
    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program.
    #[serde(with = "short_vec")]
    pub accounts: Vec<u8>,
    /// The program input data.
    #[serde(with = "short_vec")]
    pub data: Vec<u8>,
}

impl Sanitize for CompiledInstruction {}

impl CompiledInstruction {
    pub fn new<T: Serialize>(program_ids_index: u8, data: &T, accounts: Vec<u8>) -> Self {
        let data = serialize(data).unwrap();
        Self {
            program_id_index: program_ids_index,
            accounts,
            data,
        }
    }

    pub fn new_from_raw_parts(program_id_index: u8, data: Vec<u8>, accounts: Vec<u8>) -> Self {
        Self {
            program_id_index,
            accounts,
            data,
        }
    }

    pub fn program_id<'a>(&self, program_ids: &'a [Pubkey]) -> &'a Pubkey {
        &program_ids[self.program_id_index as usize]
    }
}

/// Use to query and convey information about the sibling instruction components
/// when calling the `sol_get_processed_sibling_instruction` syscall.
#[repr(C)]
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub struct ProcessedSiblingInstruction {
    /// Length of the instruction data
    pub data_len: u64,
    /// Number of AccountMeta structures
    pub accounts_len: u64,
}

/// Returns a sibling instruction from the processed sibling instruction list.
///
/// The processed sibling instruction list is a reverse-ordered list of
/// successfully processed sibling instructions. For example, given the call flow:
///
/// A
/// B -> C -> D
/// B -> E
/// B -> F
///
/// Then B's processed sibling instruction list is: `[A]`
/// Then F's processed sibling instruction list is: `[E, C]`
pub fn get_processed_sibling_instruction(index: usize) -> Option<Instruction> {
    #[cfg(target_os = "solana")]
    {
        let mut meta = ProcessedSiblingInstruction::default();
        let mut program_id = Pubkey::default();

        if 1 == unsafe {
            crate::syscalls::sol_get_processed_sibling_instruction(
                index as u64,
                &mut meta,
                &mut program_id,
                &mut u8::default(),
                &mut AccountMeta::default(),
            )
        } {
            let mut data = Vec::new();
            let mut accounts = Vec::new();
            data.resize_with(meta.data_len as usize, u8::default);
            accounts.resize_with(meta.accounts_len as usize, AccountMeta::default);

            let _ = unsafe {
                crate::syscalls::sol_get_processed_sibling_instruction(
                    index as u64,
                    &mut meta,
                    &mut program_id,
                    data.as_mut_ptr(),
                    accounts.as_mut_ptr(),
                )
            };

            Some(Instruction::new_with_bytes(program_id, &data, accounts))
        } else {
            None
        }
    }

    #[cfg(not(target_os = "solana"))]
    crate::program_stubs::sol_get_processed_sibling_instruction(index)
}

// Stack height when processing transaction-level instructions
pub const TRANSACTION_LEVEL_STACK_HEIGHT: usize = 1;

/// Get the current stack height, transaction-level instructions are height
/// TRANSACTION_LEVEL_STACK_HEIGHT, fist invoked inner instruction is height
/// TRANSACTION_LEVEL_STACK_HEIGHT + 1, etc...
pub fn get_stack_height() -> usize {
    #[cfg(target_os = "solana")]
    unsafe {
        crate::syscalls::sol_get_stack_height() as usize
    }

    #[cfg(not(target_os = "solana"))]
    {
        crate::program_stubs::sol_get_stack_height() as usize
    }
}

#[test]
fn test_account_meta_layout() {
    #[derive(Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
    struct AccountMetaRust {
        pub pubkey: Pubkey,
        pub is_signer: bool,
        pub is_writable: bool,
    }

    let account_meta_rust = AccountMetaRust::default();
    let base_rust_addr = &account_meta_rust as *const _ as u64;
    let pubkey_rust_addr = &account_meta_rust.pubkey as *const _ as u64;
    let is_signer_rust_addr = &account_meta_rust.is_signer as *const _ as u64;
    let is_writable_rust_addr = &account_meta_rust.is_writable as *const _ as u64;

    let account_meta_c = AccountMeta::default();
    let base_c_addr = &account_meta_c as *const _ as u64;
    let pubkey_c_addr = &account_meta_c.pubkey as *const _ as u64;
    let is_signer_c_addr = &account_meta_c.is_signer as *const _ as u64;
    let is_writable_c_addr = &account_meta_c.is_writable as *const _ as u64;

    assert_eq!(
        std::mem::size_of::<AccountMetaRust>(),
        std::mem::size_of::<AccountMeta>()
    );
    assert_eq!(
        pubkey_rust_addr - base_rust_addr,
        pubkey_c_addr - base_c_addr
    );
    assert_eq!(
        is_signer_rust_addr - base_rust_addr,
        is_signer_c_addr - base_c_addr
    );
    assert_eq!(
        is_writable_rust_addr - base_rust_addr,
        is_writable_c_addr - base_c_addr
    );
}
