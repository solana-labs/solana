//! @brief Example Rust-based BPF program that prints out the parameters passed to it

extern crate solana_sdk_bpf_utils;
use byteorder::{ByteOrder, LittleEndian};
use solana_sdk_bpf_utils::entrypoint::*;
use solana_sdk_bpf_utils::{entrypoint, info};

entrypoint!(process_instruction);
fn process_instruction(ka: &mut [SolKeyedAccount], _info: &SolClusterInfo, _data: &[u8]) -> bool {
    let tick_height = LittleEndian::read_u64(ka[2].data);
    assert_eq!(10u64, tick_height);

    info!("Success");
    true
}
