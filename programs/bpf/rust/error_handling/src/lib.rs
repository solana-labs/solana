//! @brief Example Rust-based BPF program that exercises error handling

extern crate solana_sdk;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use solana_sdk::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    info,
    program_error::{PrintProgramError, ProgramError},
    program_utils::DecodeError,
    pubkey::Pubkey,
};
use thiserror::Error;

/// Custom program errors
#[derive(Error, Debug, Clone, PartialEq, FromPrimitive)]
pub enum MyError {
    #[error("Default enum start")]
    DefaultEnumStart,
    #[error("The Answer")]
    TheAnswer = 42,
}
impl From<MyError> for ProgramError {
    fn from(e: MyError) -> Self {
        ProgramError::CustomError(e as u32)
    }
}
impl<T> DecodeError<T> for MyError {
    fn type_of() -> &'static str {
        "MyError"
    }
}
impl PrintProgramError for MyError {
    fn print<E>(&self)
    where
        E: 'static + std::error::Error + DecodeError<E> + PrintProgramError + FromPrimitive,
    {
        match self {
            MyError::DefaultEnumStart => info!("Error: Default enum start"),
            MyError::TheAnswer => info!("Error: The Answer"),
        }
    }
}

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data[0] {
        1 => {
            info!("return success");
            Ok(())
        }
        2 => {
            info!("return a builtin");
            Err(ProgramError::InvalidAccountData)
        }
        3 => {
            info!("return default enum start value");
            Err(MyError::DefaultEnumStart.into())
        }
        4 => {
            info!("return custom error");
            Err(MyError::TheAnswer.into())
        }
        6 => {
            let data = accounts[0].try_borrow_mut_data()?;
            let data2 = accounts[0].try_borrow_mut_data()?;
            assert_eq!(*data, *data2);
            Ok(())
        }
        _ => {
            info!("Unrecognized command");
            Err(ProgramError::InvalidInstructionData)
        }
    }
}
