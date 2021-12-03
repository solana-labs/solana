//! Example Rust-based BPF program that tests rand behavior

#![allow(unreachable_code)]

use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, msg, pubkey::Pubkey,
};

entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo<'_>],
    _instruction_data: &[u8],
) -> ProgramResult {
    msg!("rand");
    Ok(())
}
