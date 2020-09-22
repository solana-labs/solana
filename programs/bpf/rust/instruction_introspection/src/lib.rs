//! @brief Example Rust-based BPF program that exercises instruction introspection

extern crate solana_sdk;
use solana_sdk::{
    account_info::next_account_info, account_info::AccountInfo, entrypoint,
    entrypoint::ProgramResult, info, program_error::ProgramError, pubkey::Pubkey,
    sysvar::instructions,
};

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.is_empty() {
        return Err(ProgramError::InvalidAccountData);
    }

    let secp_instruction_index = instruction_data[0];
    let account_info_iter = &mut accounts.iter();
    let instruction_accounts = next_account_info(account_info_iter)?;
    assert_eq!(*instruction_accounts.key, instructions::id());
    let data_len = instruction_accounts.try_borrow_data()?.len();
    if data_len < 2 {
        return Err(ProgramError::InvalidAccountData);
    }

    let instruction = instructions::get_instruction(
        secp_instruction_index as usize,
        &instruction_accounts.try_borrow_data()?,
    )
    .map_err(|_| ProgramError::InvalidAccountData)?;

    let current_instruction =
        instructions::get_current_instruction(&instruction_accounts.try_borrow_data()?);
    let my_index = instruction_data[1] as u16;
    assert_eq!(current_instruction, my_index);

    info!(&format!("id: {}", instruction.program_id));

    info!(&format!("data[0]: {}", instruction.data[0]));
    info!(&format!("index: {}", current_instruction));
    Ok(())
}
