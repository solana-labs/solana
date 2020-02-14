//! Mapping functions to Instruction errors

use log::*;
use solana_sdk::instruction::InstructionError;
use std::convert::TryInto;
use types::vm_error::{StatusCode, VMStatus};
use vm::file_format::CompiledModule;

#[allow(clippy::needless_pass_by_value)]
pub fn map_data_error(err: std::boxed::Box<bincode::ErrorKind>) -> InstructionError {
    debug!("Error: Account data: {:?}", err);
    match err.as_ref() {
        bincode::ErrorKind::SizeLimit => InstructionError::AccountDataTooSmall,
        _ => InstructionError::InvalidAccountData,
    }
}

#[allow(clippy::needless_pass_by_value)]
pub fn map_json_error(err: serde_json::error::Error) -> InstructionError {
    debug!("Error: serde_json: {:?}", err);
    InstructionError::InvalidAccountData
}

pub fn map_vm_verification_error(err: (CompiledModule, Vec<VMStatus>)) -> InstructionError {
    debug!("Error: Script verification failed: {:?}", err.1);
    InstructionError::InvalidInstructionData
}

#[allow(clippy::needless_pass_by_value)]
pub fn missing_account() -> InstructionError {
    debug!("Error: Missing account");
    InstructionError::InvalidAccountData
}

pub fn map_failure_error(err: failure::Error) -> InstructionError {
    debug!("Error: Script verification failed: {:?}", err);
    InstructionError::InvalidInstructionData
}

pub fn map_err_vm_status(status: VMStatus) -> InstructionError {
    // Attempt to map the StatusCode (repr(u64)) to a u32 for CustomError.
    // The only defined StatusCode that fails is StatusCode::UNKNOWN_ERROR
    match <StatusCode as Into<u64>>::into(status.major_status).try_into() {
        Ok(u) => InstructionError::CustomError(u),
        Err(_) => InstructionError::InvalidError,
    }
}
