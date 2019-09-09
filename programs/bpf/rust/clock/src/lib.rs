//! @brief Example Rust-based BPF program that prints out the parameters passed to it

extern crate solana_sdk;
use solana_sdk::{
    account_info::AccountInfo, entrypoint, entrypoint::SUCCESS, info, pubkey::Pubkey,
    sysvar::clock::Clock,
};

entrypoint!(process_instruction);
fn process_instruction(_program_id: &Pubkey, accounts: &mut [AccountInfo], _data: &[u8]) -> u32 {
    match Clock::from(&accounts[2]) {
        Some(clock) => {
            info!("slot, segment, epoch, stakers_epoch");
            info!(
                clock.slot,
                clock.segment, clock.epoch, clock.stakers_epoch, 0
            );
            assert_eq!(clock.slot, 42);
        }
        None => {
            info!("Failed to get clock from account 2");
            panic!();
        }
    }

    info!("Success");
    SUCCESS
}
