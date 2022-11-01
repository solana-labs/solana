//! Example Rust-based SBF program that tests sysvar use

extern crate solana_program;
#[allow(deprecated)]
use solana_program::sysvar::fees::Fees;
#[allow(deprecated)]
use solana_program::sysvar::recent_blockhashes::RecentBlockhashes;
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{
        self, clock::Clock, epoch_schedule::EpochSchedule, instructions, rent::Rent,
        slot_hashes::SlotHashes, slot_history::SlotHistory, stake_history::StakeHistory, Sysvar,
    },
};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
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
    assert_eq!(*accounts[4].owner, sysvar::id());
    let index = instructions::load_current_index_checked(&accounts[4])?;
    let instruction = instructions::load_instruction_at_checked(index as usize, &accounts[4])?;
    assert_eq!(0, index);
    assert_eq!(
        instruction,
        Instruction::new_with_bytes(
            *program_id,
            instruction_data,
            vec![
                AccountMeta::new(*accounts[0].key, true),
                AccountMeta::new(*accounts[1].key, false),
                AccountMeta::new_readonly(*accounts[2].key, false),
                AccountMeta::new_readonly(*accounts[3].key, false),
                AccountMeta::new_readonly(*accounts[4].key, false),
                AccountMeta::new_readonly(*accounts[5].key, false),
                AccountMeta::new_readonly(*accounts[6].key, false),
                AccountMeta::new_readonly(*accounts[7].key, false),
                AccountMeta::new_readonly(*accounts[8].key, false),
                AccountMeta::new_readonly(*accounts[9].key, false),
                AccountMeta::new_readonly(*accounts[10].key, false),
            ],
        )
    );

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

    // Fees
    #[allow(deprecated)]
    if instruction_data[0] == 1 {
        msg!("Fee identifier:");
        sysvar::fees::id().log();
        let fees = Fees::from_account_info(&accounts[10]).unwrap();
        let got_fees = Fees::get()?;
        assert_eq!(fees, got_fees);
    }

    Ok(())
}
