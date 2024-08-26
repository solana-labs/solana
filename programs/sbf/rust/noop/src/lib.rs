//! Example Rust-based SBF noop program

extern crate solana_program;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

// This program intentionally uses `entrypoint!` instead of `entrypoint_no_alloc!`
// to handle any number of accounts.
solana_program::entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    Ok(())
}
