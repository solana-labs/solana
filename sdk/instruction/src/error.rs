use core::fmt;
#[cfg(feature = "frozen-abi")]
use solana_frozen_abi_macro::{AbiEnumVisitor, AbiExample};
#[cfg(feature = "std")]
use {
    num_traits::ToPrimitive,
    std::string::{String, ToString},
};

/// Builtin return values occupy the upper 32 bits
const BUILTIN_BIT_SHIFT: usize = 32;
macro_rules! to_builtin {
    ($error:expr) => {
        ($error as u64) << BUILTIN_BIT_SHIFT
    };
}

pub const CUSTOM_ZERO: u64 = to_builtin!(1);
pub const INVALID_ARGUMENT: u64 = to_builtin!(2);
pub const INVALID_INSTRUCTION_DATA: u64 = to_builtin!(3);
pub const INVALID_ACCOUNT_DATA: u64 = to_builtin!(4);
pub const ACCOUNT_DATA_TOO_SMALL: u64 = to_builtin!(5);
pub const INSUFFICIENT_FUNDS: u64 = to_builtin!(6);
pub const INCORRECT_PROGRAM_ID: u64 = to_builtin!(7);
pub const MISSING_REQUIRED_SIGNATURES: u64 = to_builtin!(8);
pub const ACCOUNT_ALREADY_INITIALIZED: u64 = to_builtin!(9);
pub const UNINITIALIZED_ACCOUNT: u64 = to_builtin!(10);
pub const NOT_ENOUGH_ACCOUNT_KEYS: u64 = to_builtin!(11);
pub const ACCOUNT_BORROW_FAILED: u64 = to_builtin!(12);
pub const MAX_SEED_LENGTH_EXCEEDED: u64 = to_builtin!(13);
pub const INVALID_SEEDS: u64 = to_builtin!(14);
pub const BORSH_IO_ERROR: u64 = to_builtin!(15);
pub const ACCOUNT_NOT_RENT_EXEMPT: u64 = to_builtin!(16);
pub const UNSUPPORTED_SYSVAR: u64 = to_builtin!(17);
pub const ILLEGAL_OWNER: u64 = to_builtin!(18);
pub const MAX_ACCOUNTS_DATA_ALLOCATIONS_EXCEEDED: u64 = to_builtin!(19);
pub const INVALID_ACCOUNT_DATA_REALLOC: u64 = to_builtin!(20);
pub const MAX_INSTRUCTION_TRACE_LENGTH_EXCEEDED: u64 = to_builtin!(21);
pub const BUILTIN_PROGRAMS_MUST_CONSUME_COMPUTE_UNITS: u64 = to_builtin!(22);
pub const INVALID_ACCOUNT_OWNER: u64 = to_builtin!(23);
pub const ARITHMETIC_OVERFLOW: u64 = to_builtin!(24);
pub const IMMUTABLE: u64 = to_builtin!(25);
pub const INCORRECT_AUTHORITY: u64 = to_builtin!(26);
// Warning: Any new error codes added here must also be:
// - Added to the below conversions
// - Added as an equivalent to ProgramError and InstructionError
// - Be featurized in the BPF loader to return `InstructionError::InvalidError`
//   until the feature is activated

/// Reasons the runtime might have rejected an instruction.
///
/// Members of this enum must not be removed, but new ones can be added.
/// Also, it is crucial that meta-information if any that comes along with
/// an error be consistent across software versions.  For example, it is
/// dangerous to include error strings from 3rd party crates because they could
/// change at any time and changes to them are difficult to detect.
#[cfg(feature = "std")]
#[cfg_attr(feature = "frozen-abi", derive(AbiExample, AbiEnumVisitor))]
#[cfg_attr(
    feature = "serde",
    derive(serde_derive::Serialize, serde_derive::Deserialize)
)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum InstructionError {
    /// Deprecated! Use CustomError instead!
    /// The program instruction returned an error
    GenericError,

    /// The arguments provided to a program were invalid
    InvalidArgument,

    /// An instruction's data contents were invalid
    InvalidInstructionData,

    /// An account's data contents was invalid
    InvalidAccountData,

    /// An account's data was too small
    AccountDataTooSmall,

    /// An account's balance was too small to complete the instruction
    InsufficientFunds,

    /// The account did not have the expected program id
    IncorrectProgramId,

    /// A signature was required but not found
    MissingRequiredSignature,

    /// An initialize instruction was sent to an account that has already been initialized.
    AccountAlreadyInitialized,

    /// An attempt to operate on an account that hasn't been initialized.
    UninitializedAccount,

    /// Program's instruction lamport balance does not equal the balance after the instruction
    UnbalancedInstruction,

    /// Program illegally modified an account's program id
    ModifiedProgramId,

    /// Program spent the lamports of an account that doesn't belong to it
    ExternalAccountLamportSpend,

    /// Program modified the data of an account that doesn't belong to it
    ExternalAccountDataModified,

    /// Read-only account's lamports modified
    ReadonlyLamportChange,

    /// Read-only account's data was modified
    ReadonlyDataModified,

    /// An account was referenced more than once in a single instruction
    // Deprecated, instructions can now contain duplicate accounts
    DuplicateAccountIndex,

    /// Executable bit on account changed, but shouldn't have
    ExecutableModified,

    /// Rent_epoch account changed, but shouldn't have
    RentEpochModified,

    /// The instruction expected additional account keys
    NotEnoughAccountKeys,

    /// Program other than the account's owner changed the size of the account data
    AccountDataSizeChanged,

    /// The instruction expected an executable account
    AccountNotExecutable,

    /// Failed to borrow a reference to account data, already borrowed
    AccountBorrowFailed,

    /// Account data has an outstanding reference after a program's execution
    AccountBorrowOutstanding,

    /// The same account was multiply passed to an on-chain program's entrypoint, but the program
    /// modified them differently.  A program can only modify one instance of the account because
    /// the runtime cannot determine which changes to pick or how to merge them if both are modified
    DuplicateAccountOutOfSync,

    /// Allows on-chain programs to implement program-specific error types and see them returned
    /// by the Solana runtime. A program-specific error may be any type that is represented as
    /// or serialized to a u32 integer.
    Custom(u32),

    /// The return value from the program was invalid.  Valid errors are either a defined builtin
    /// error value or a user-defined error in the lower 32 bits.
    InvalidError,

    /// Executable account's data was modified
    ExecutableDataModified,

    /// Executable account's lamports modified
    ExecutableLamportChange,

    /// Executable accounts must be rent exempt
    ExecutableAccountNotRentExempt,

    /// Unsupported program id
    UnsupportedProgramId,

    /// Cross-program invocation call depth too deep
    CallDepth,

    /// An account required by the instruction is missing
    MissingAccount,

    /// Cross-program invocation reentrancy not allowed for this instruction
    ReentrancyNotAllowed,

    /// Length of the seed is too long for address generation
    MaxSeedLengthExceeded,

    /// Provided seeds do not result in a valid address
    InvalidSeeds,

    /// Failed to reallocate account data of this length
    InvalidRealloc,

    /// Computational budget exceeded
    ComputationalBudgetExceeded,

    /// Cross-program invocation with unauthorized signer or writable account
    PrivilegeEscalation,

    /// Failed to create program execution environment
    ProgramEnvironmentSetupFailure,

    /// Program failed to complete
    ProgramFailedToComplete,

    /// Program failed to compile
    ProgramFailedToCompile,

    /// Account is immutable
    Immutable,

    /// Incorrect authority provided
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
    BorshIoError(String),

    /// An account does not have enough lamports to be rent-exempt
    AccountNotRentExempt,

    /// Invalid account owner
    InvalidAccountOwner,

    /// Program arithmetic overflowed
    ArithmeticOverflow,

    /// Unsupported sysvar
    UnsupportedSysvar,

    /// Illegal account owner
    IllegalOwner,

    /// Accounts data allocations exceeded the maximum allowed per transaction
    MaxAccountsDataAllocationsExceeded,

    /// Max accounts exceeded
    MaxAccountsExceeded,

    /// Max instruction trace length exceeded
    MaxInstructionTraceLengthExceeded,

    /// Builtin programs must consume compute units
    BuiltinProgramsMustConsumeComputeUnits,
    // Note: For any new error added here an equivalent ProgramError and its
    // conversions must also be added
}

#[cfg(feature = "std")]
impl std::error::Error for InstructionError {}

#[cfg(feature = "std")]
impl fmt::Display for InstructionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InstructionError::GenericError => f.write_str("generic instruction error"),
            InstructionError::InvalidArgument => f.write_str("invalid program argument"),
            InstructionError::InvalidInstructionData => f.write_str("invalid instruction data"),
            InstructionError::InvalidAccountData => {
                f.write_str("invalid account data for instruction")
            }
            InstructionError::AccountDataTooSmall => {
                f.write_str("account data too small for instruction")
            }
            InstructionError::InsufficientFunds => {
                f.write_str("insufficient funds for instruction")
            }
            InstructionError::IncorrectProgramId => {
                f.write_str("incorrect program id for instruction")
            }
            InstructionError::MissingRequiredSignature => {
                f.write_str("missing required signature for instruction")
            }
            InstructionError::AccountAlreadyInitialized => {
                f.write_str("instruction requires an uninitialized account")
            }
            InstructionError::UninitializedAccount => {
                f.write_str("instruction requires an initialized account")
            }
            InstructionError::UnbalancedInstruction => {
                f.write_str("sum of account balances before and after instruction do not match")
            }
            InstructionError::ModifiedProgramId => {
                f.write_str("instruction illegally modified the program id of an account")
            }
            InstructionError::ExternalAccountLamportSpend => {
                f.write_str("instruction spent from the balance of an account it does not own")
            }
            InstructionError::ExternalAccountDataModified => {
                f.write_str("instruction modified data of an account it does not own")
            }
            InstructionError::ReadonlyLamportChange => {
                f.write_str("instruction changed the balance of a read-only account")
            }
            InstructionError::ReadonlyDataModified => {
                f.write_str("instruction modified data of a read-only account")
            }
            InstructionError::DuplicateAccountIndex => {
                f.write_str("instruction contains duplicate accounts")
            }
            InstructionError::ExecutableModified => {
                f.write_str("instruction changed executable bit of an account")
            }
            InstructionError::RentEpochModified => {
                f.write_str("instruction modified rent epoch of an account")
            }
            InstructionError::NotEnoughAccountKeys => {
                f.write_str("insufficient account keys for instruction")
            }
            InstructionError::AccountDataSizeChanged => f.write_str(
                "program other than the account's owner changed the size of the account data",
            ),
            InstructionError::AccountNotExecutable => {
                f.write_str("instruction expected an executable account")
            }
            InstructionError::AccountBorrowFailed => f.write_str(
                "instruction tries to borrow reference for an account which is already borrowed",
            ),
            InstructionError::AccountBorrowOutstanding => {
                f.write_str("instruction left account with an outstanding borrowed reference")
            }
            InstructionError::DuplicateAccountOutOfSync => {
                f.write_str("instruction modifications of multiply-passed account differ")
            }
            InstructionError::Custom(num) => {
                write!(f, "custom program error: {num:#x}")
            }
            InstructionError::InvalidError => f.write_str("program returned invalid error code"),
            InstructionError::ExecutableDataModified => {
                f.write_str("instruction changed executable accounts data")
            }
            InstructionError::ExecutableLamportChange => {
                f.write_str("instruction changed the balance of an executable account")
            }
            InstructionError::ExecutableAccountNotRentExempt => {
                f.write_str("executable accounts must be rent exempt")
            }
            InstructionError::UnsupportedProgramId => f.write_str("Unsupported program id"),
            InstructionError::CallDepth => {
                f.write_str("Cross-program invocation call depth too deep")
            }
            InstructionError::MissingAccount => {
                f.write_str("An account required by the instruction is missing")
            }
            InstructionError::ReentrancyNotAllowed => {
                f.write_str("Cross-program invocation reentrancy not allowed for this instruction")
            }
            InstructionError::MaxSeedLengthExceeded => {
                f.write_str("Length of the seed is too long for address generation")
            }
            InstructionError::InvalidSeeds => {
                f.write_str("Provided seeds do not result in a valid address")
            }
            InstructionError::InvalidRealloc => f.write_str("Failed to reallocate account data"),
            InstructionError::ComputationalBudgetExceeded => {
                f.write_str("Computational budget exceeded")
            }
            InstructionError::PrivilegeEscalation => {
                f.write_str("Cross-program invocation with unauthorized signer or writable account")
            }
            InstructionError::ProgramEnvironmentSetupFailure => {
                f.write_str("Failed to create program execution environment")
            }
            InstructionError::ProgramFailedToComplete => f.write_str("Program failed to complete"),
            InstructionError::ProgramFailedToCompile => f.write_str("Program failed to compile"),
            InstructionError::Immutable => f.write_str("Account is immutable"),
            InstructionError::IncorrectAuthority => f.write_str("Incorrect authority provided"),
            InstructionError::BorshIoError(s) => {
                write!(f, "Failed to serialize or deserialize account data: {s}",)
            }
            InstructionError::AccountNotRentExempt => {
                f.write_str("An account does not have enough lamports to be rent-exempt")
            }
            InstructionError::InvalidAccountOwner => f.write_str("Invalid account owner"),
            InstructionError::ArithmeticOverflow => f.write_str("Program arithmetic overflowed"),
            InstructionError::UnsupportedSysvar => f.write_str("Unsupported sysvar"),
            InstructionError::IllegalOwner => f.write_str("Provided owner is not allowed"),
            InstructionError::MaxAccountsDataAllocationsExceeded => f.write_str(
                "Accounts data allocations exceeded the maximum allowed per transaction",
            ),
            InstructionError::MaxAccountsExceeded => f.write_str("Max accounts exceeded"),
            InstructionError::MaxInstructionTraceLengthExceeded => {
                f.write_str("Max instruction trace length exceeded")
            }
            InstructionError::BuiltinProgramsMustConsumeComputeUnits => {
                f.write_str("Builtin programs must consume compute units")
            }
        }
    }
}

#[cfg(feature = "std")]
impl<T> From<T> for InstructionError
where
    T: ToPrimitive,
{
    fn from(error: T) -> Self {
        let error = error.to_u64().unwrap_or(0xbad_c0de);
        match error {
            CUSTOM_ZERO => Self::Custom(0),
            INVALID_ARGUMENT => Self::InvalidArgument,
            INVALID_INSTRUCTION_DATA => Self::InvalidInstructionData,
            INVALID_ACCOUNT_DATA => Self::InvalidAccountData,
            ACCOUNT_DATA_TOO_SMALL => Self::AccountDataTooSmall,
            INSUFFICIENT_FUNDS => Self::InsufficientFunds,
            INCORRECT_PROGRAM_ID => Self::IncorrectProgramId,
            MISSING_REQUIRED_SIGNATURES => Self::MissingRequiredSignature,
            ACCOUNT_ALREADY_INITIALIZED => Self::AccountAlreadyInitialized,
            UNINITIALIZED_ACCOUNT => Self::UninitializedAccount,
            NOT_ENOUGH_ACCOUNT_KEYS => Self::NotEnoughAccountKeys,
            ACCOUNT_BORROW_FAILED => Self::AccountBorrowFailed,
            MAX_SEED_LENGTH_EXCEEDED => Self::MaxSeedLengthExceeded,
            INVALID_SEEDS => Self::InvalidSeeds,
            BORSH_IO_ERROR => Self::BorshIoError("Unknown".to_string()),
            ACCOUNT_NOT_RENT_EXEMPT => Self::AccountNotRentExempt,
            UNSUPPORTED_SYSVAR => Self::UnsupportedSysvar,
            ILLEGAL_OWNER => Self::IllegalOwner,
            MAX_ACCOUNTS_DATA_ALLOCATIONS_EXCEEDED => Self::MaxAccountsDataAllocationsExceeded,
            INVALID_ACCOUNT_DATA_REALLOC => Self::InvalidRealloc,
            MAX_INSTRUCTION_TRACE_LENGTH_EXCEEDED => Self::MaxInstructionTraceLengthExceeded,
            BUILTIN_PROGRAMS_MUST_CONSUME_COMPUTE_UNITS => {
                Self::BuiltinProgramsMustConsumeComputeUnits
            }
            INVALID_ACCOUNT_OWNER => Self::InvalidAccountOwner,
            ARITHMETIC_OVERFLOW => Self::ArithmeticOverflow,
            IMMUTABLE => Self::Immutable,
            INCORRECT_AUTHORITY => Self::IncorrectAuthority,
            _ => {
                // A valid custom error has no bits set in the upper 32
                if error >> BUILTIN_BIT_SHIFT == 0 {
                    Self::Custom(error as u32)
                } else {
                    Self::InvalidError
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum LamportsError {
    /// arithmetic underflowed
    ArithmeticUnderflow,
    /// arithmetic overflowed
    ArithmeticOverflow,
}

#[cfg(feature = "std")]
impl std::error::Error for LamportsError {}

impl fmt::Display for LamportsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ArithmeticUnderflow => f.write_str("Arithmetic underflowed"),
            Self::ArithmeticOverflow => f.write_str("Arithmetic overflowed"),
        }
    }
}

#[cfg(feature = "std")]
impl From<LamportsError> for InstructionError {
    fn from(error: LamportsError) -> Self {
        match error {
            LamportsError::ArithmeticOverflow => InstructionError::ArithmeticOverflow,
            LamportsError::ArithmeticUnderflow => InstructionError::ArithmeticOverflow,
        }
    }
}
