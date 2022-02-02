use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    pubkey::Pubkey,
    sysvar::Sysvar,
};
use std::convert::TryInto;

entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let slot_account = next_account_info(account_info_iter)?;
    let incinerator_account = next_account_info(account_info_iter)?;

    // Slot is an u64 at the end of the structure
    let data = slot_account.data.borrow();
    let slot: u64 = u64::from_le_bytes(data[data.len() - 8..].try_into().unwrap());

    let clock = Clock::get().unwrap();

    msg!("next_slot is {:?} ", slot);
    msg!("clock is in slot {:?} ", clock.slot);

    msg!("incinerator balance: {:?}", incinerator_account.lamports());

    if clock.slot >= slot {
        msg!("We are onchain, let's get the money");
    } else {
        panic!("We are in a simulation, let's credit the victim !");
    }
    Ok(())
}
