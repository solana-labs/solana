//! Defines an Instruction error type

use crate::system_error::SystemError;

/// Reasons the runtime might have rejected an instruction.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum InstructionError {
    /// Deprecated! Use CustomError instead!
    /// The program instruction returned an error
    GenericError,

    /// The arguments provided to a program instruction where invalid
    InvalidArgument,

    /// An instruction's data contents was invalid
    InvalidInstructionData,

    /// An account's data contents was invalid
    InvalidAccountData,

    /// An account's data was too small
    AccountDataTooSmall,

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

    /// Program modified an account's program id
    ModifiedProgramId,

    /// Program spent the lamports of an account that doesn't belong to it
    ExternalAccountLamportSpend,

    /// Program modified the data of an account that doesn't belong to it
    ExternalAccountDataModified,

    /// An account was referenced more than once in a single instruction
    DuplicateAccountIndex,

    /// CustomError allows on-chain programs to implement program-specific error types and see
    /// them returned by the Solana runtime. A CustomError may be any type that is serialized
    /// to a Vec of bytes, max length 32 bytes. Any CustomError Vec greater than this length will
    /// be truncated by the runtime.
    CustomError(Vec<u8>),
}

impl InstructionError {
    pub fn new_result_with_negative_lamports() -> Self {
        let serialized_error =
            bincode::serialize(&SystemError::ResultWithNegativeLamports).unwrap();
        InstructionError::CustomError(serialized_error)
    }
}
