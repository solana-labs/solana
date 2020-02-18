#![cfg(feature = "program")]

use crate::{
    account_info::AccountInfo, entrypoint::ProgramResult, entrypoint::SUCCESS,
    instruction::Instruction,
};

/// Invoke a cross-program instruction
pub fn process_instruction(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
) -> ProgramResult {
    process_signed_instruction(instruction, account_infos, &[])
}

/// Invoke a cross-program instruction with program signatures
pub fn process_signed_instruction(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&str]],
) -> ProgramResult {
    let result = unsafe {
        sol_process_signed_instruction_(
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
    fn sol_process_signed_instruction_(
        instruction_addr: *const u8,
        account_infos_addr: *const u8,
        account_infos_len: u64,
        signers_seeds_addr: *const u8,
        signers_seeds_len: u64,
    ) -> u64;
}
