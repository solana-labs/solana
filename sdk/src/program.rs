#![cfg(feature = "program")]

use crate::{
    account_info::AccountInfo, entrypoint::ProgramResult, entrypoint::SUCCESS,
    instruction::Instruction, program_error::ProgramError, pubkey::Pubkey,
};

pub fn create_program_address(
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<Pubkey, ProgramError> {
    let bytes = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];
    let result = unsafe {
        sol_create_program_address(
            seeds as *const _ as *const u8,
            seeds.len() as u64,
            program_id as *const _ as *const u8,
            &bytes as *const _ as *const u8,
        )
    };
    match result {
        SUCCESS => Ok(Pubkey::new(&bytes)),
        _ => Err(result.into()),
    }
}
extern "C" {
    fn sol_create_program_address(
        seeds_addr: *const u8,
        seeds_len: u64,
        program_id_addr: *const u8,
        address_bytes_addr: *const u8,
    ) -> u64;
}

/// Invoke a cross-program instruction
pub fn invoke(instruction: &Instruction, account_infos: &[AccountInfo]) -> ProgramResult {
    invoke_signed(instruction, account_infos, &[])
}

/// Invoke a cross-program instruction with program signatures
pub fn invoke_signed(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&[u8]]],
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
