//! @brief Example Rust-based BPF program that tests sysvar use

extern crate solana_program;
#[allow(deprecated)]
use solana_program::sysvar::recent_blockhashes::RecentBlockhashes;
use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{
        self, clock::Clock, epoch_schedule::EpochSchedule, instructions, rent::Rent,
        slot_hashes::SlotHashes, slot_history::SlotHistory, stake_history::StakeHistory, Sysvar,
    },
};

entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Clock
    {
        msg!("Clock identifier:");
        sysvar::clock::id().log();
        let clock = Clock::from_account_info(&accounts[2]).unwrap();
        assert_ne!(clock, Clock::default());
        let got_clock = Clock::get()?;
        assert_eq!(clock, got_clock);
    }

    // Epoch Schedule
    {
        msg!("EpochSchedule identifier:");
        sysvar::epoch_schedule::id().log();
        let epoch_schedule = EpochSchedule::from_account_info(&accounts[3]).unwrap();
        assert_eq!(epoch_schedule, EpochSchedule::default());
        let got_epoch_schedule = EpochSchedule::get()?;
        assert_eq!(epoch_schedule, got_epoch_schedule);
    }

    // Instructions
    msg!("Instructions identifier:");
    sysvar::instructions::id().log();
    let index = instructions::load_current_index(&accounts[4].try_borrow_data()?);
    assert_eq!(0, index);

    // Recent Blockhashes
    #[allow(deprecated)]
    {
        msg!("RecentBlockhashes identifier:");
        sysvar::recent_blockhashes::id().log();
        let recent_blockhashes = RecentBlockhashes::from_account_info(&accounts[5]).unwrap();
        assert_ne!(recent_blockhashes, RecentBlockhashes::default());
    }

    // Rent
    {
        msg!("Rent identifier:");
        sysvar::rent::id().log();
        let rent = Rent::from_account_info(&accounts[6]).unwrap();
        assert_eq!(rent, Rent::default());
        let got_rent = Rent::get()?;
        assert_eq!(rent, got_rent);
    }

    // Slot Hashes
    msg!("SlotHashes identifier:");
    sysvar::slot_hashes::id().log();
    assert_eq!(
        Err(ProgramError::UnsupportedSysvar),
        SlotHashes::from_account_info(&accounts[7])
    );

    // Slot History
    msg!("SlotHistory identifier:");
    sysvar::slot_history::id().log();
    assert_eq!(
        Err(ProgramError::UnsupportedSysvar),
        SlotHistory::from_account_info(&accounts[8])
    );

    // Stake History
    msg!("StakeHistory identifier:");
    sysvar::stake_history::id().log();
    let _ = StakeHistory::from_account_info(&accounts[9]).unwrap();

    Ok(())
}
