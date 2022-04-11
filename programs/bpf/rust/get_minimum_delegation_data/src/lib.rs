//! Example/test program for calling GetMinimumDelegation and then the
//! helper function to return the minimum delegation value.

#![allow(unreachable_code)]

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program, pubkey::Pubkey, stake,
};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let get_minimum_delegation_instruction = stake::instruction::get_minimum_delegation();
    program::invoke(&get_minimum_delegation_instruction, &[]).unwrap();

    let minimum_delegation = stake::instruction::utils::get_minimum_delegation_data();
    assert!(minimum_delegation.is_some());
    Ok(())
}
