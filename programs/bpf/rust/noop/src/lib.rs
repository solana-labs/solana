//! @brief Example Rust-based BPF noop program

extern crate solana_sdk;
use solana_sdk::{
    account_info::AccountInfo, bpf_loader, entrypoint, entrypoint::ProgramResult, info, log::*,
    pubkey::Pubkey,
};

entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    Ok(())
}
