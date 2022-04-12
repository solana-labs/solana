//! Example/test program for calling GetMinimumDelegation and then the
//! helper function to return the minimum delegation value.

#![allow(unreachable_code)]

extern crate solana_program;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey, stake};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let minimum_delegation = stake::tools::get_minimum_delegation();
    assert!(minimum_delegation.is_some());
    Ok(())
}
