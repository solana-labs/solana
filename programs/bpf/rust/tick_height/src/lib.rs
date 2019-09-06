//! @brief Example Rust-based BPF program that prints out the parameters passed to it

extern crate solana_sdk;
use byteorder::{ByteOrder, LittleEndian};
use solana_sdk::entrypoint::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::{entrypoint, info};

entrypoint!(process_instruction);
fn process_instruction(_program_id: &Pubkey, ka: &mut [SolKeyedAccount], _data: &[u8]) -> u32 {
    let tick_height = LittleEndian::read_u64(ka[2].data);
    assert_eq!(10u64, tick_height);

    info!("Success");
    SUCCESS
}
