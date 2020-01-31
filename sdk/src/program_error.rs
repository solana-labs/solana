use crate::instruction::InstructionError;
use num_traits::ToPrimitive;

/// Reasons the program may fail
pub enum ProgramError {
    /// CustomError allows programs to implement program-specific error types and see
    /// them returned by the Solana runtime. A CustomError may be any type that is represented
    /// as or serialized to a u32 integer.
    ///
    /// NOTE: u64 requires special serialization to avoid the loss of precision in JS clients and
    /// so is not used for now.
    CustomError(u32),
    /// The arguments provided to a program instruction where invalid
    InvalidArgument,
    /// An instruction's data contents was invalid
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
    /// The instruction expected additional account keys
    NotEnoughAccountKeys,
    /// Failed to borrow a reference to account data, already borrowed
    AccountBorrowFailed,
}

/// 32bit representations of builtin program errors returned by the entry point
const BUILTIN_ERROR_START: u32 = 0x8000_0000; // 31st bit set
const INVALID_ARGUMENT: u32 = BUILTIN_ERROR_START;
const INVALID_INSTRUCTION_DATA: u32 = BUILTIN_ERROR_START + 1;
const INVALID_ACCOUNT_DATA: u32 = BUILTIN_ERROR_START + 2;
const ACCOUNT_DATA_TOO_SMALL: u32 = BUILTIN_ERROR_START + 3;
const INSUFFICIENT_FUNDS: u32 = BUILTIN_ERROR_START + 4;
const INCORRECT_PROGRAM_ID: u32 = BUILTIN_ERROR_START + 5;
const MISSING_REQUIRED_SIGNATURES: u32 = BUILTIN_ERROR_START + 6;
const ACCOUNT_ALREADY_INITIALIZED: u32 = BUILTIN_ERROR_START + 7;
const UNINITIALIZED_ACCOUNT: u32 = BUILTIN_ERROR_START + 8;
const NOT_ENOUGH_ACCOUNT_KEYS: u32 = BUILTIN_ERROR_START + 9;
const ACCOUNT_BORROW_FAILED: u32 = BUILTIN_ERROR_START + 10;

/// Is this a builtin error? (is 31th bit set?)
fn is_builtin(error: u32) -> bool {
    (error & BUILTIN_ERROR_START) != 0
}

/// If a program defined error conflicts with a builtin error
/// its 30th bit is set before returning to distinguish it.
/// The side effect is that the original error's 30th bit
/// value is lost, be aware.
const CONFLICTING_ERROR_MARK: u32 = 0x4000_0000; // 30st bit set

/// Is this error marked as conflicting? (is 30th bit set?)
fn is_marked_conflicting(error: u32) -> bool {
    (error & CONFLICTING_ERROR_MARK) != 0
}

/// Mark as a conflicting error
fn mark_conflicting(error: u32) -> u32 {
    error | CONFLICTING_ERROR_MARK
}

/// Unmark as a conflicting error
fn unmark_conflicting(error: u32) -> u32 {
    error & !CONFLICTING_ERROR_MARK
}

impl From<ProgramError> for u32 {
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
            ProgramError::CustomError(error) => {
                if error == 0 || is_builtin(error) {
                    mark_conflicting(error)
                } else {
                    error
                }
            }
        }
    }
}

impl<T> From<T> for InstructionError
where
    T: ToPrimitive,
{
    fn from(error: T) -> Self {
        let error = error.to_u32().unwrap_or(0xbad_c0de);
        match error {
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
            _ => {
                if is_marked_conflicting(error) {
                    InstructionError::ConflictingError(unmark_conflicting(error))
                } else {
                    InstructionError::CustomError(error)
                }
            }
        }
    }
}
