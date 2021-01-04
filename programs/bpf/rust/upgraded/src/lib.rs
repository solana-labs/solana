//! @brief Example Rust-based BPF upgraded program

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    pubkey::Pubkey,
    sysvar::{clock, fees},
};

entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    msg!("Upgraded program");
    assert_eq!(accounts.len(), 3);
    assert_eq!(accounts[0].key, program_id);
    assert_eq!(*accounts[1].key, clock::id());
    assert_eq!(*accounts[2].key, fees::id());
    Err(43.into())
}
