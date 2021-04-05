//! @brief Example Rust-based BPF program that tests sysvar use

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo,
    clock::DEFAULT_SLOTS_PER_EPOCH,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent,
    sysvar::{
        self, clock::Clock, fees::Fees, instructions, rent::Rent, slot_hashes::SlotHashes,
        slot_history::SlotHistory, stake_history::StakeHistory, Sysvar,
    },
};

entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Clock
    msg!("Clock identifier:");
    sysvar::clock::id().log();
    let clock = Clock::from_account_info(&accounts[2]).unwrap();
    assert_eq!(clock.slot, DEFAULT_SLOTS_PER_EPOCH + 1);

    // Fees
    msg!("Fees identifier:");
    sysvar::fees::id().log();
    let fees = Fees::from_account_info(&accounts[3]).unwrap();
    let fee_calculator = fees.fee_calculator;
    assert_eq!(fee_calculator.lamports_per_signature, 0);

    // Instructions
    msg!("Instructions identifier:");
    sysvar::instructions::id().log();
    let index = instructions::load_current_index(&accounts[4].try_borrow_data()?);
    assert_eq!(0, index);
    msg!(
        "instruction {:?}",
        instructions::load_instruction_at(index as usize, &accounts[4].try_borrow_data()?)
    );

    let due = Rent::from_account_info(&accounts[5]).unwrap().due(
        rent::DEFAULT_LAMPORTS_PER_BYTE_YEAR * rent::DEFAULT_EXEMPTION_THRESHOLD as u64,
        1,
        1.0,
    );
    assert_eq!(due, (0, true));

    // Slot Hashes
    msg!("SlotHashes identifier:");
    sysvar::slot_hashes::id().log();
    assert_eq!(
        Err(ProgramError::UnsupportedSysvar),
        SlotHashes::from_account_info(&accounts[6])
    );

    // Slot History
    msg!("SlotHistory identifier:");
    sysvar::slot_history::id().log();
    assert_eq!(
        Err(ProgramError::UnsupportedSysvar),
        SlotHistory::from_account_info(&accounts[7])
    );

    // Stake History
    msg!("StakeHistory identifier:");
    sysvar::stake_history::id().log();
    let stake_history = StakeHistory::from_account_info(&accounts[8]).unwrap();
    assert!(stake_history.len() >= 1);

    Ok(())
}
