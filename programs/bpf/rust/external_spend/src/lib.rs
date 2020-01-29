//! @brief Example Rust-based BPF program that moves a lamport from one account to another

extern crate solana_sdk;
use solana_sdk::{account_info::AccountInfo, entrypoint, entrypoint::SUCCESS, pubkey::Pubkey};

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> u32 {
    // account 0 is the mint and not owned by this program, any debit of its lamports
    // should result in a failed program execution.  Test to ensure that this debit
    // is seen by the runtime and fails as expected
    **accounts[0].lamports.borrow_mut() -= 1;

    SUCCESS
}
