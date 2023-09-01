#![allow(clippy::arithmetic_side_effects)]

use {
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::Clock,
        declare_id,
        entrypoint::ProgramResult,
        msg,
        pubkey::Pubkey,
        sysvar::Sysvar,
    },
    std::convert::TryInto,
};

declare_id!("Sim1jD5C35odT8mzctm8BWnjic8xW5xgeb5MbcbErTo");

solana_program::entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let slot_history_account_info = next_account_info(account_info_iter)?;
    let clock_account_info = next_account_info(account_info_iter)?;

    // Slot is an u64 at the end of the structure
    let data = slot_history_account_info.data.borrow();
    let slot: u64 = u64::from_le_bytes(data[data.len() - 8..].try_into().unwrap());

    let clock_from_cache = Clock::get().unwrap();
    let clock_from_account = Clock::from_account_info(clock_account_info).unwrap();

    msg!("next_slot from slot history is {:?} ", slot);
    msg!("clock from cache is in slot {:?} ", clock_from_cache.slot);
    msg!(
        "clock from account is in slot {:?} ",
        clock_from_account.slot
    );
    if clock_from_cache.slot >= slot {
        msg!("On-chain");
    } else {
        panic!("Simulation");
    }

    if clock_from_cache.slot != clock_from_account.slot {
        panic!("Simulation");
    }

    Ok(())
}
