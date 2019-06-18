//! @brief Example Rust-based BPF program that prints out the parameters passed to it

#![no_std]
#![allow(unreachable_code)]

extern crate solana_sdk_bpf_utils;

use byteorder::{ByteOrder, LittleEndian};
use solana_sdk_bpf_utils::entrypoint;
use solana_sdk_bpf_utils::entrypoint::*;
use solana_sdk_bpf_utils::log::*;

entrypoint!(process_instruction);
fn process_instruction(
    ka: &mut [Option<SolKeyedAccount>; MAX_ACCOUNTS],
    _info: &SolClusterInfo,
    _data: &[u8],
) -> bool {
    sol_log("Tick Height:");
    if let Some(k) = &ka[2] {
        let tick_height = LittleEndian::read_u64(k.data);
        assert_eq!(10u64, tick_height);
        sol_log("Success");
        return true
    }
    panic!();
}
