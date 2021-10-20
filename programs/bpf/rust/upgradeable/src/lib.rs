//! Example Rust-based BPF upgradeable program

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, msg, pubkey::Pubkey,
    sysvar::clock,
};

entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    msg!("Upgradeable program");
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].key, program_id);
    assert_eq!(*accounts[1].key, clock::id());
    Err(42.into())
}
