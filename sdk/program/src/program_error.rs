use crate::{decode_error::DecodeError, instruction::InstructionError, msg, pubkey::PubkeyError};
use num_traits::{FromPrimitive, ToPrimitive};
use std::convert::TryFrom;
use thiserror::Error;

/// Reasons the program may fail
#[derive(Clone, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
pub enum ProgramError {
    /// Allows on-chain programs to implement program-specific error types and see them returned
    /// by the Solana runtime. A program-specific error may be any type that is represented as
    /// or serialized to a u32 integer.
    #[error("Custom program error: {0:#x}")]
    Custom(u32),
    #[error("The arguments provided to a program instruction where invalid")]
    InvalidArgument,
    #[error("An instruction's data contents was invalid")]
    InvalidInstructionData,
    #[error("An account's data contents was invalid")]
    InvalidAccountData,
    #[error("An account's data was too small")]
    AccountDataTooSmall,
    #[error("An account's balance was too small to complete the instruction")]
    InsufficientFunds,
    #[error("The account did not have the expected program id")]
    IncorrectProgramId,
    #[error("A signature was required but not found")]
    MissingRequiredSignature,
    #[error("An initialize instruction was sent to an account that has already been initialized")]
    AccountAlreadyInitialized,
    #[error("An attempt to operate on an account that hasn't been initialized")]
    UninitializedAccount,
    #[error("The instruction expected additional account keys")]
    NotEnoughAccountKeys,
    #[error("Failed to borrow a reference to account data, already borrowed")]
    AccountBorrowFailed,
    #[error("Length of the seed is too long for address generation")]
    MaxSeedLengthExceeded,
    #[error("Provided seeds do not result in a valid address")]
    InvalidSeeds,
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
        }
    }
}

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
            INVALID_ARGUMENT => ProgramError::InvalidArgument,
            INVALID_INSTRUCTION_DATA => ProgramError::InvalidInstructionData,
            INVALID_ACCOUNT_DATA => ProgramError::InvalidAccountData,
            ACCOUNT_DATA_TOO_SMALL => ProgramError::AccountDataTooSmall,
            INSUFFICIENT_FUNDS => ProgramError::InsufficientFunds,
            INCORRECT_PROGRAM_ID => ProgramError::IncorrectProgramId,
            MISSING_REQUIRED_SIGNATURES => ProgramError::MissingRequiredSignature,
            ACCOUNT_ALREADY_INITIALIZED => ProgramError::AccountAlreadyInitialized,
            UNINITIALIZED_ACCOUNT => ProgramError::UninitializedAccount,
            NOT_ENOUGH_ACCOUNT_KEYS => ProgramError::NotEnoughAccountKeys,
            ACCOUNT_BORROW_FAILED => ProgramError::AccountBorrowFailed,
            MAX_SEED_LENGTH_EXCEEDED => ProgramError::MaxSeedLengthExceeded,
            INVALID_SEEDS => ProgramError::InvalidSeeds,
            CUSTOM_ZERO => ProgramError::Custom(0),
            _ => ProgramError::Custom(error as u32),
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
            _ => Err(error),
        }
    }
}

impl<T> From<T> for InstructionError
where
    T: ToPrimitive,
{
    fn from(error: T) -> Self {
        let error = error.to_u64().unwrap_or(0xbad_c0de);
        match error {
            CUSTOM_ZERO => InstructionError::Custom(0),
            INVALID_ARGUMENT => InstructionError::InvalidArgument,
            INVALID_INSTRUCTION_DATA => InstructionError::InvalidInstructionData,
            INVALID_ACCOUNT_DATA => InstructionError::InvalidAccountData,
            ACCOUNT_DATA_TOO_SMALL => InstructionError::AccountDataTooSmall,
            INSUFFICIENT_FUNDS => InstructionError::InsufficientFunds,
            INCORRECT_PROGRAM_ID => InstructionError::IncorrectProgramId,
            MISSING_REQUIRED_SIGNATURES => InstructionError::MissingRequiredSignature,
            ACCOUNT_ALREADY_INITIALIZED => InstructionError::AccountAlreadyInitialized,
            UNINITIALIZED_ACCOUNT => InstructionError::UninitializedAccount,
            NOT_ENOUGH_ACCOUNT_KEYS => InstructionError::NotEnoughAccountKeys,
            ACCOUNT_BORROW_FAILED => InstructionError::AccountBorrowFailed,
            MAX_SEED_LENGTH_EXCEEDED => InstructionError::MaxSeedLengthExceeded,
            INVALID_SEEDS => InstructionError::InvalidSeeds,
            _ => {
                // A valid custom error has no bits set in the upper 32
                if error >> BUILTIN_BIT_SHIFT == 0 {
                    InstructionError::Custom(error as u32)
                } else {
                    Self::InvalidError
                }
            }
        }
    }
}

impl From<PubkeyError> for ProgramError {
    fn from(error: PubkeyError) -> Self {
        match error {
            PubkeyError::MaxSeedLengthExceeded => ProgramError::MaxSeedLengthExceeded,
            PubkeyError::InvalidSeeds => ProgramError::InvalidSeeds,
        }
    }
}
