//! @brief Example Rust-based BPF program that tests sysval use

extern crate solana_sdk;
use solana_sdk::{
    account_info::AccountInfo,
    clock::{get_segment_from_slot, DEFAULT_SLOTS_PER_EPOCH, DEFAULT_SLOTS_PER_SEGMENT},
    entrypoint,
    entrypoint::SUCCESS,
    pubkey::Pubkey,
    rent,
    sysvar::{
        clock::Clock, fees::Fees, rent::Rent, rewards::Rewards, slot_hashes::SlotHashes,
        stake_history::StakeHistory,
    },
};

entrypoint!(process_instruction);
fn process_instruction(_program_id: &Pubkey, accounts: &mut [AccountInfo], _data: &[u8]) -> u32 {
    // Clock
    let clock = Clock::from_account_info(&accounts[2]).expect("clock");
    assert_eq!(clock.slot, DEFAULT_SLOTS_PER_EPOCH + 1);
    assert_eq!(
        clock.segment,
        get_segment_from_slot(clock.slot, DEFAULT_SLOTS_PER_SEGMENT)
    );

    // Fees
    let fees = Fees::from_account_info(&accounts[3]).expect("fees");
    let burn = fees.fee_calculator.burn(42);
    assert_eq!(burn, (21, 21));

    // Rewards
    let _rewards = Rewards::from_account_info(&accounts[4]).expect("rewards");

    // Slot Hashes
    let slot_hashes = SlotHashes::from_account_info(&accounts[5]).expect("slot_hashes");
    assert!(slot_hashes.len() >= 1);

    // Stake History
    let stake_history = StakeHistory::from_account_info(&accounts[6]).expect("stake_history");
    assert!(stake_history.len() >= 1);

    let rent = Rent::from_account_info(&accounts[7]).unwrap();
    assert_eq!(
        rent.due(
            rent::DEFAULT_LAMPORTS_PER_BYTE_YEAR * rent::DEFAULT_EXEMPTION_THRESHOLD as u64,
            1,
            1.0
        ),
        (0, true)
    );

    SUCCESS
}
