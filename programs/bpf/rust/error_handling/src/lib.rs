//! @brief Example Rust-based BPF program that exercises error handling

extern crate solana_sdk;
use num_derive::FromPrimitive;
use solana_sdk::{
    account_info::AccountInfo, entrypoint, info, program_error::ProgramError, pubkey::Pubkey,
};
use thiserror::Error;

/// Custom program errors
#[derive(Error, Debug, Clone, PartialEq, FromPrimitive)]
// Clippy compains about 0x8000_002d, but we don't care about C compatibility here
#[allow(clippy::enum_clike_unportable_variant)]
pub enum MyError {
    #[error("The Answer")]
    TheAnswer = 42,
    #[error("Conflicting with success")]
    ConflictingSuccess = 0,
    #[error("Conflicting with builtin")]
    ConflictingBuiltin = 0x8000_002d,
}
impl From<MyError> for ProgramError {
    fn from(e: MyError) -> Self {
        ProgramError::CustomError(e as u32)
    }
}

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> Result<(), ProgramError> {
    match instruction_data[0] {
        1 => {
            info!("return success");
            Ok(())
        }
        2 => {
            info!("return a builtin");
            Err(ProgramError::AccountBorrowFailed)
        }
        3 => {
            info!("return custom error");
            Err(MyError::TheAnswer.into())
        }
        4 => {
            info!("return error that conflicts with success");
            Err(MyError::ConflictingSuccess.into())
        }
        5 => {
            info!("return error that conflicts with builtin");
            Err(MyError::ConflictingBuiltin.into())
        }
        _ => {
            info!("Unrecognized command");
            Err(ProgramError::InvalidInstructionData)
        }
    }
}
