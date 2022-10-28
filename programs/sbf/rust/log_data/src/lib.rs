//! Example Rust-based SBF program that uses sol_log_data syscall

#![cfg(feature = "program")]

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, log::sol_log_data,
    program::set_return_data, pubkey::Pubkey,
};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::cognitive_complexity)]
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let fields: Vec<&[u8]> = instruction_data.split(|e| *e == 0).collect();

    set_return_data(&[0x08, 0x01, 0x44]);

    sol_log_data(&fields);

    Ok(())
}
