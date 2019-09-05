//! @brief Example Rust-based BPF program that prints out the parameters passed to it

extern crate solana_sdk_bpf;
use solana_sdk_bpf::account::Account;
use solana_sdk_bpf::pubkey::Pubkey;
use solana_sdk_bpf::sysvar::clock::Clock;
use solana_sdk_bpf::{entrypoint, info};

entrypoint!(process_instruction);
fn process_instruction(_program_id: &Pubkey, accounts: &mut [Account], _data: &[u8]) -> bool {
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
    true
}
