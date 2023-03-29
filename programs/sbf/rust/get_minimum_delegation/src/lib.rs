//! Example/test program to get the minimum stake delegation via the helper function

#![allow(unreachable_code)]

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, msg, pubkey::Pubkey, stake,
};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let minimum_delegation = stake::tools::get_minimum_delegation()?;
    msg!(
        "The minimum stake delegation is {} lamports",
        minimum_delegation
    );
    Ok(())
}
