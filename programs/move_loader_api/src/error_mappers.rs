//! Mapping functions to Instruction errors

use log::*;
use solana_sdk::instruction::InstructionError;
use vm::file_format::CompiledModule;

#[allow(clippy::needless_pass_by_value)]
pub fn map_vm_runtime_error(err: vm::errors::VMRuntimeError) -> InstructionError {
    debug!("Execution failed: {:?}", err);
    match err.err {
        vm::errors::VMErrorKind::OutOfGasError => InstructionError::InsufficientFunds,
        _ => InstructionError::GenericError,
    }
}
pub fn map_vm_invariant_violation_error(err: vm::errors::VMInvariantViolation) -> InstructionError {
    debug!("Error: Execution failed: {:?}", err);
    InstructionError::GenericError
}
pub fn map_vm_binary_error(err: vm::errors::BinaryError) -> InstructionError {
    debug!("Error: Script deserialize failed: {:?}", err);
    InstructionError::InvalidInstructionData
}
#[allow(clippy::needless_pass_by_value)]
pub fn map_data_error(err: std::boxed::Box<bincode::ErrorKind>) -> InstructionError {
    debug!("Error: Account data: {:?}", err);
    InstructionError::InvalidAccountData
}
pub fn map_vm_verification_error(
    err: (CompiledModule, Vec<vm::errors::VerificationError>),
) -> InstructionError {
    debug!("Error: Script verification failed: {:?}", err.1);
    InstructionError::InvalidInstructionData
}
pub fn map_failure_error(err: failure::Error) -> InstructionError {
    debug!("Error: Script verification failed: {:?}", err);
    InstructionError::InvalidInstructionData
}
#[allow(clippy::needless_pass_by_value)]
pub fn missing_account() -> InstructionError {
    debug!("Error: Missing account");
    InstructionError::InvalidAccountData
}
