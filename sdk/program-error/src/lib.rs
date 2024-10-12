//! The [`ProgramError`] type and related definitions.

#![allow(clippy::arithmetic_side_effects)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#[cfg(feature = "borsh")]
use borsh::io::Error as BorshIoError;
#[cfg(feature = "serde")]
use serde_derive::{Deserialize, Serialize};
use {
    core::fmt,
    num_traits::FromPrimitive,
    solana_decode_error::DecodeError,
    solana_instruction::error::{
        InstructionError, ACCOUNT_ALREADY_INITIALIZED, ACCOUNT_BORROW_FAILED,
        ACCOUNT_DATA_TOO_SMALL, ACCOUNT_NOT_RENT_EXEMPT, ARITHMETIC_OVERFLOW, BORSH_IO_ERROR,
        BUILTIN_PROGRAMS_MUST_CONSUME_COMPUTE_UNITS, CUSTOM_ZERO, ILLEGAL_OWNER, IMMUTABLE,
        INCORRECT_AUTHORITY, INCORRECT_PROGRAM_ID, INSUFFICIENT_FUNDS, INVALID_ACCOUNT_DATA,
        INVALID_ACCOUNT_DATA_REALLOC, INVALID_ACCOUNT_OWNER, INVALID_ARGUMENT,
        INVALID_INSTRUCTION_DATA, INVALID_SEEDS, MAX_ACCOUNTS_DATA_ALLOCATIONS_EXCEEDED,
        MAX_INSTRUCTION_TRACE_LENGTH_EXCEEDED, MAX_SEED_LENGTH_EXCEEDED,
        MISSING_REQUIRED_SIGNATURES, NOT_ENOUGH_ACCOUNT_KEYS, UNINITIALIZED_ACCOUNT,
        UNSUPPORTED_SYSVAR,
    },
    solana_msg::msg,
    solana_pubkey::PubkeyError,
    std::convert::TryFrom,
};

pub type ProgramResult = std::result::Result<(), ProgramError>;

/// Reasons the program may fail
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramError {
    /// Allows on-chain programs to implement program-specific error types and see them returned
    /// by the Solana runtime. A program-specific error may be any type that is represented as
    /// or serialized to a u32 integer.
    Custom(u32),
    InvalidArgument,
    InvalidInstructionData,
    InvalidAccountData,
    AccountDataTooSmall,
    InsufficientFunds,
    IncorrectProgramId,
    MissingRequiredSignature,
    AccountAlreadyInitialized,
    UninitializedAccount,
    NotEnoughAccountKeys,
    AccountBorrowFailed,
    MaxSeedLengthExceeded,
    InvalidSeeds,
    BorshIoError(String),
    AccountNotRentExempt,
    UnsupportedSysvar,
    IllegalOwner,
    MaxAccountsDataAllocationsExceeded,
    InvalidRealloc,
    MaxInstructionTraceLengthExceeded,
    BuiltinProgramsMustConsumeComputeUnits,
    InvalidAccountOwner,
    ArithmeticOverflow,
    Immutable,
    IncorrectAuthority,
}

impl std::error::Error for ProgramError {}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProgramError::Custom(num) => write!(f,"Custom program error: {num:#x}"),
            ProgramError::InvalidArgument
             => f.write_str("The arguments provided to a program instruction were invalid"),
            ProgramError::InvalidInstructionData
             => f.write_str("An instruction's data contents was invalid"),
            ProgramError::InvalidAccountData
             => f.write_str("An account's data contents was invalid"),
            ProgramError::AccountDataTooSmall
             => f.write_str("An account's data was too small"),
            ProgramError::InsufficientFunds
             => f.write_str("An account's balance was too small to complete the instruction"),
            ProgramError::IncorrectProgramId
             => f.write_str("The account did not have the expected program id"),
            ProgramError::MissingRequiredSignature
             => f.write_str("A signature was required but not found"),
            ProgramError::AccountAlreadyInitialized
             => f.write_str("An initialize instruction was sent to an account that has already been initialized"),
            ProgramError::UninitializedAccount
             => f.write_str("An attempt to operate on an account that hasn't been initialized"),
            ProgramError::NotEnoughAccountKeys
             => f.write_str("The instruction expected additional account keys"),
            ProgramError::AccountBorrowFailed
             => f.write_str("Failed to borrow a reference to account data, already borrowed"),
            ProgramError::MaxSeedLengthExceeded
             => f.write_str("Length of the seed is too long for address generation"),
            ProgramError::InvalidSeeds
             => f.write_str("Provided seeds do not result in a valid address"),
            ProgramError::BorshIoError(s) =>  write!(f, "IO Error: {s}"),
            ProgramError::AccountNotRentExempt
             => f.write_str("An account does not have enough lamports to be rent-exempt"),
            ProgramError::UnsupportedSysvar
             => f.write_str("Unsupported sysvar"),
            ProgramError::IllegalOwner
             => f.write_str("Provided owner is not allowed"),
            ProgramError::MaxAccountsDataAllocationsExceeded
             => f.write_str("Accounts data allocations exceeded the maximum allowed per transaction"),
            ProgramError::InvalidRealloc
             => f.write_str("Account data reallocation was invalid"),
            ProgramError::MaxInstructionTraceLengthExceeded
             => f.write_str("Instruction trace length exceeded the maximum allowed per transaction"),
            ProgramError::BuiltinProgramsMustConsumeComputeUnits
             => f.write_str("Builtin programs must consume compute units"),
            ProgramError::InvalidAccountOwner
             => f.write_str("Invalid account owner"),
            ProgramError::ArithmeticOverflow
             => f.write_str("Program arithmetic overflowed"),
            ProgramError::Immutable
             => f.write_str("Account is immutable"),
            ProgramError::IncorrectAuthority
             => f.write_str("Incorrect authority provided"),
        }
    }
}

pub trait PrintProgramError {
    fn print<E>(&self)
    where
        E: 'static + std::error::Error + DecodeError<E> + PrintProgramError + FromPrimitive;
}

impl PrintProgramError for ProgramError {
    fn print<E>(&self)
    where
        E: 'static + std::error::Error + DecodeError<E> + PrintProgramError + FromPrimitive,
    {
        match self {
            Self::Custom(error) => {
                if let Some(custom_error) = E::decode_custom_error_to_enum(*error) {
                    custom_error.print::<E>();
                } else {
                    msg!("Error: Unknown");
                }
            }
            Self::InvalidArgument => msg!("Error: InvalidArgument"),
            Self::InvalidInstructionData => msg!("Error: InvalidInstructionData"),
            Self::InvalidAccountData => msg!("Error: InvalidAccountData"),
            Self::AccountDataTooSmall => msg!("Error: AccountDataTooSmall"),
            Self::InsufficientFunds => msg!("Error: InsufficientFunds"),
            Self::IncorrectProgramId => msg!("Error: IncorrectProgramId"),
            Self::MissingRequiredSignature => msg!("Error: MissingRequiredSignature"),
            Self::AccountAlreadyInitialized => msg!("Error: AccountAlreadyInitialized"),
            Self::UninitializedAccount => msg!("Error: UninitializedAccount"),
            Self::NotEnoughAccountKeys => msg!("Error: NotEnoughAccountKeys"),
            Self::AccountBorrowFailed => msg!("Error: AccountBorrowFailed"),
            Self::MaxSeedLengthExceeded => msg!("Error: MaxSeedLengthExceeded"),
            Self::InvalidSeeds => msg!("Error: InvalidSeeds"),
            Self::BorshIoError(_) => msg!("Error: BorshIoError"),
            Self::AccountNotRentExempt => msg!("Error: AccountNotRentExempt"),
            Self::UnsupportedSysvar => msg!("Error: UnsupportedSysvar"),
            Self::IllegalOwner => msg!("Error: IllegalOwner"),
            Self::MaxAccountsDataAllocationsExceeded => {
                msg!("Error: MaxAccountsDataAllocationsExceeded")
            }
            Self::InvalidRealloc => msg!("Error: InvalidRealloc"),
            Self::MaxInstructionTraceLengthExceeded => {
                msg!("Error: MaxInstructionTraceLengthExceeded")
            }
            Self::BuiltinProgramsMustConsumeComputeUnits => {
                msg!("Error: BuiltinProgramsMustConsumeComputeUnits")
            }
            Self::InvalidAccountOwner => msg!("Error: InvalidAccountOwner"),
            Self::ArithmeticOverflow => msg!("Error: ArithmeticOverflow"),
            Self::Immutable => msg!("Error: Immutable"),
            Self::IncorrectAuthority => msg!("Error: IncorrectAuthority"),
        }
    }
}

impl From<ProgramError> for u64 {
    fn from(error: ProgramError) -> Self {
        match error {
            ProgramError::InvalidArgument => INVALID_ARGUMENT,
            ProgramError::InvalidInstructionData => INVALID_INSTRUCTION_DATA,
            ProgramError::InvalidAccountData => INVALID_ACCOUNT_DATA,
            ProgramError::AccountDataTooSmall => ACCOUNT_DATA_TOO_SMALL,
            ProgramError::InsufficientFunds => INSUFFICIENT_FUNDS,
            ProgramError::IncorrectProgramId => INCORRECT_PROGRAM_ID,
            ProgramError::MissingRequiredSignature => MISSING_REQUIRED_SIGNATURES,
            ProgramError::AccountAlreadyInitialized => ACCOUNT_ALREADY_INITIALIZED,
            ProgramError::UninitializedAccount => UNINITIALIZED_ACCOUNT,
            ProgramError::NotEnoughAccountKeys => NOT_ENOUGH_ACCOUNT_KEYS,
            ProgramError::AccountBorrowFailed => ACCOUNT_BORROW_FAILED,
            ProgramError::MaxSeedLengthExceeded => MAX_SEED_LENGTH_EXCEEDED,
            ProgramError::InvalidSeeds => INVALID_SEEDS,
            ProgramError::BorshIoError(_) => BORSH_IO_ERROR,
            ProgramError::AccountNotRentExempt => ACCOUNT_NOT_RENT_EXEMPT,
            ProgramError::UnsupportedSysvar => UNSUPPORTED_SYSVAR,
            ProgramError::IllegalOwner => ILLEGAL_OWNER,
            ProgramError::MaxAccountsDataAllocationsExceeded => {
                MAX_ACCOUNTS_DATA_ALLOCATIONS_EXCEEDED
            }
            ProgramError::InvalidRealloc => INVALID_ACCOUNT_DATA_REALLOC,
            ProgramError::MaxInstructionTraceLengthExceeded => {
                MAX_INSTRUCTION_TRACE_LENGTH_EXCEEDED
            }
            ProgramError::BuiltinProgramsMustConsumeComputeUnits => {
                BUILTIN_PROGRAMS_MUST_CONSUME_COMPUTE_UNITS
            }
            ProgramError::InvalidAccountOwner => INVALID_ACCOUNT_OWNER,
            ProgramError::ArithmeticOverflow => ARITHMETIC_OVERFLOW,
            ProgramError::Immutable => IMMUTABLE,
            ProgramError::IncorrectAuthority => INCORRECT_AUTHORITY,
            ProgramError::Custom(error) => {
                if error == 0 {
                    CUSTOM_ZERO
                } else {
                    error as u64
                }
            }
        }
    }
}

impl From<u64> for ProgramError {
    fn from(error: u64) -> Self {
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
            _ => Self::Custom(error as u32),
        }
    }
}

impl TryFrom<InstructionError> for ProgramError {
    type Error = InstructionError;

    fn try_from(error: InstructionError) -> Result<Self, Self::Error> {
        match error {
            Self::Error::Custom(err) => Ok(Self::Custom(err)),
            Self::Error::InvalidArgument => Ok(Self::InvalidArgument),
            Self::Error::InvalidInstructionData => Ok(Self::InvalidInstructionData),
            Self::Error::InvalidAccountData => Ok(Self::InvalidAccountData),
            Self::Error::AccountDataTooSmall => Ok(Self::AccountDataTooSmall),
            Self::Error::InsufficientFunds => Ok(Self::InsufficientFunds),
            Self::Error::IncorrectProgramId => Ok(Self::IncorrectProgramId),
            Self::Error::MissingRequiredSignature => Ok(Self::MissingRequiredSignature),
            Self::Error::AccountAlreadyInitialized => Ok(Self::AccountAlreadyInitialized),
            Self::Error::UninitializedAccount => Ok(Self::UninitializedAccount),
            Self::Error::NotEnoughAccountKeys => Ok(Self::NotEnoughAccountKeys),
            Self::Error::AccountBorrowFailed => Ok(Self::AccountBorrowFailed),
            Self::Error::MaxSeedLengthExceeded => Ok(Self::MaxSeedLengthExceeded),
            Self::Error::InvalidSeeds => Ok(Self::InvalidSeeds),
            Self::Error::BorshIoError(err) => Ok(Self::BorshIoError(err)),
            Self::Error::AccountNotRentExempt => Ok(Self::AccountNotRentExempt),
            Self::Error::UnsupportedSysvar => Ok(Self::UnsupportedSysvar),
            Self::Error::IllegalOwner => Ok(Self::IllegalOwner),
            Self::Error::MaxAccountsDataAllocationsExceeded => {
                Ok(Self::MaxAccountsDataAllocationsExceeded)
            }
            Self::Error::InvalidRealloc => Ok(Self::InvalidRealloc),
            Self::Error::MaxInstructionTraceLengthExceeded => {
                Ok(Self::MaxInstructionTraceLengthExceeded)
            }
            Self::Error::BuiltinProgramsMustConsumeComputeUnits => {
                Ok(Self::BuiltinProgramsMustConsumeComputeUnits)
            }
            Self::Error::InvalidAccountOwner => Ok(Self::InvalidAccountOwner),
            Self::Error::ArithmeticOverflow => Ok(Self::ArithmeticOverflow),
            Self::Error::Immutable => Ok(Self::Immutable),
            Self::Error::IncorrectAuthority => Ok(Self::IncorrectAuthority),
            _ => Err(error),
        }
    }
}

impl From<PubkeyError> for ProgramError {
    fn from(error: PubkeyError) -> Self {
        match error {
            PubkeyError::MaxSeedLengthExceeded => Self::MaxSeedLengthExceeded,
            PubkeyError::InvalidSeeds => Self::InvalidSeeds,
            PubkeyError::IllegalOwner => Self::IllegalOwner,
        }
    }
}

#[cfg(feature = "borsh")]
impl From<BorshIoError> for ProgramError {
    fn from(error: BorshIoError) -> Self {
        Self::BorshIoError(format!("{error}"))
    }
}
