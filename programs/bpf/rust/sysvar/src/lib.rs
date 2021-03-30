//! @brief Example Rust-based BPF program that tests sysvar use

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo,
    clock::DEFAULT_SLOTS_PER_EPOCH,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    pubkey::Pubkey,
    rent,
    sysvar::{
        self, clock::Clock, fees::Fees, rent::Rent, slot_hashes::SlotHashes,
        stake_history::StakeHistory, Sysvar,
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
    let clock = Clock::from_account_info(&accounts[2]).expect("clock");
    assert_eq!(clock.slot, DEFAULT_SLOTS_PER_EPOCH + 1);

    // Fees
    msg!("Fees identifier:");
    sysvar::fees::id().log();
    let fees = Fees::from_account_info(&accounts[3]).expect("fees");
    let fee_calculator = fees.fee_calculator;
    assert_eq!(fee_calculator.lamports_per_signature, 0);

    // Slot Hashes
    msg!("SlotHashes identifier:");
    sysvar::slot_hashes::id().log();
    let slot_hashes = SlotHashes::from_account_info(&accounts[4]).expect("slot_hashes");
    assert!(slot_hashes.len() >= 1);

    // Stake History
    msg!("StakeHistory identifier:");
    sysvar::stake_history::id().log();
    let stake_history = StakeHistory::from_account_info(&accounts[5]).expect("stake_history");
    assert!(stake_history.len() >= 1);

    let rent = Rent::from_account_info(&accounts[6]).unwrap();
    assert_eq!(
        rent.due(
            rent::DEFAULT_LAMPORTS_PER_BYTE_YEAR * rent::DEFAULT_EXEMPTION_THRESHOLD as u64,
            1,
            1.0
        ),
        (0, true)
    );

    Ok(())
}
