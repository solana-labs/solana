//! Example Rust-based SBF program that exercises error handling

extern crate solana_program;
use {
    num_derive::FromPrimitive,
    num_traits::FromPrimitive,
    solana_program::{
        account_info::AccountInfo,
        decode_error::DecodeError,
        entrypoint::ProgramResult,
        msg,
        program_error::{PrintProgramError, ProgramError},
        pubkey::{Pubkey, PubkeyError},
    },
    thiserror::Error,
};

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
        ProgramError::Custom(e as u32)
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
            MyError::DefaultEnumStart => msg!("Error: Default enum start"),
            MyError::TheAnswer => msg!("Error: The Answer"),
        }
    }
}

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data[0] {
        1 => {
            msg!("return success");
            Ok(())
        }
        2 => {
            msg!("return a builtin");
            Err(ProgramError::InvalidAccountData)
        }
        3 => {
            msg!("return default enum start value");
            Err(MyError::DefaultEnumStart.into())
        }
        4 => {
            msg!("return custom error");
            Err(MyError::TheAnswer.into())
        }
        7 => {
            let data = accounts[0].try_borrow_mut_data()?;
            let data2 = accounts[0].try_borrow_mut_data()?;
            assert_eq!(*data, *data2);
            Ok(())
        }
        9 => {
            msg!("return pubkey error");
            Err(PubkeyError::MaxSeedLengthExceeded.into())
        }
        _ => {
            msg!("Unsupported");
            Err(ProgramError::InvalidInstructionData)
        }
    }
}
