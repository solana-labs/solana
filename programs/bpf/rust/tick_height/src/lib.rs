//! @brief Example Rust-based BPF program that prints out the parameters passed to it

extern crate solana_sdk_bpf;
use byteorder::{ByteOrder, LittleEndian};
use solana_sdk_bpf::account::Account;
use solana_sdk_bpf::pubkey::Pubkey;
use solana_sdk_bpf::{entrypoint, info};

entrypoint!(process_instruction);
fn process_instruction(_program_id: &Pubkey, accounts: &mut [Account], _data: &[u8]) -> bool {
    // TODO use sysvar
    let tick_height = LittleEndian::read_u64(accounts[2].data);
    assert_eq!(10u64, tick_height);

    info!("Success");
    true
}
