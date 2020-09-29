//! @brief Example Rust-based BPF program that tests rand behavior

#![allow(unreachable_code)]

extern crate solana_sdk;
use solana_sdk::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, info, pubkey::Pubkey,
};

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    info!("rand");
    Ok(())
}
