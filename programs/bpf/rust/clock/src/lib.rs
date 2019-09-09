//! @brief Example Rust-based BPF program that prints out the parameters passed to it

extern crate solana_sdk;
use solana_sdk::{
    account_info::AccountInfo, entrypoint, entrypoint::SUCCESS, info, pubkey::Pubkey,
    sysvar::clock::Clock,
};

entrypoint!(process_instruction);
fn process_instruction(_program_id: &Pubkey, accounts: &mut [AccountInfo], _data: &[u8]) -> u32 {

    // Clock
    let clock = Clock::from_account_info(&accounts[2]).unwrap();
    info!("slot, segment, epoch, stakers_epoch");
    info!(
        clock.slot,
        clock.segment, clock.epoch, clock.stakers_epoch, 0,
    );
    assert_eq!(clock.slot, 42);

    // Fees
    let fees = Fees::from_account_info(&accounts[3]).unwrap();

    // Rewards
    let fees = Fees::from_account_info(&accounts[4]).unwrap();
    info!("validator_point_value (trunc/fract), storage_point_value (trunc/fract)");
    info!(
        rewards.validator_point_value.trunc() as u64,
        rewards.validator_point_value.fract() as u64,
        storage_point_value.trunc() as u64,
        storage_point_value.fract() as u64,
        0
    );

    // Slot Hashes
    let slot_hashes = SlotHashes::from_account_info(&accounts[5]).unwrap();
    info!("slot hash length");
    info!(slat_hashes.len(), 0, 0, 0, 0);

    // Stake History
    let stake_history = StakeHistory::from_account_info(&accounts[6]).unwrap();
    info!("stake history length");
    info!(stake_history.len(), 0, 0, 0, 0);

    info!("Success");
    SUCCESS
}
