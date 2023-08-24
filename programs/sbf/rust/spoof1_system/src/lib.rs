#![allow(clippy::arithmetic_side_effects)]

use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let from = &accounts[0];
    let to = &accounts[1];

    let to_balance = to.lamports();
    **to.lamports.borrow_mut() = to_balance + from.lamports();
    **from.lamports.borrow_mut() = 0u64;

    Ok(())
}
