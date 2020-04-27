#![cfg(feature = "program")]

use crate::{
    account_info::AccountInfo, entrypoint::ProgramResult, entrypoint::SUCCESS,
    instruction::Instruction,
};

/// Invoke a cross-program instruction
pub fn invoke(instruction: &Instruction, account_infos: &[AccountInfo]) -> ProgramResult {
    invoke_signed(instruction, account_infos, &[])
}

/// Invoke a cross-program instruction with program signatures
pub fn invoke_signed(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&str]],
) -> ProgramResult {
    let result = unsafe {
        sol_invoke_signed_rust(
            instruction as *const _ as *const u8,
            account_infos as *const _ as *const u8,
            account_infos.len() as u64,
            signers_seeds as *const _ as *const u8,
            signers_seeds.len() as u64,
        )
    };
    match result {
        SUCCESS => Ok(()),
        _ => Err(result.into()),
    }
}

extern "C" {
    fn sol_invoke_signed_rust(
        instruction_addr: *const u8,
        account_infos_addr: *const u8,
        account_infos_len: u64,
        signers_seeds_addr: *const u8,
        signers_seeds_len: u64,
    ) -> u64;
}
