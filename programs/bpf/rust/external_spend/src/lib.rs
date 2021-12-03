//! Example Rust-based BPF program that moves a lamport from one account to another

use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
};

entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    _instruction_data: &[u8],
) -> ProgramResult {
    // account 0 is the mint and not owned by this program, any debit of its lamports
    // should result in a failed program execution.  Test to ensure that this debit
    // is seen by the runtime and fails as expected
    **accounts[0].lamports.borrow_mut() -= 1;

    Ok(())
}
