//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

#[macro_use]
extern crate alloc;
#[cfg(not(test))]
extern crate solana_sdk_bpf_no_std;
extern crate solana_sdk_bpf_utils;

use alloc::vec::Vec;
use solana_sdk_bpf_utils::info;
use solana_sdk_bpf_utils::entrypoint;
use solana_sdk_bpf_utils::entrypoint::{SolKeyedAccount, SolClusterInfo};
use solana_bpf_rust_stack_bug_dep::{InitPollData, PollData};

entrypoint!(process_instruction);
fn process_instruction(ka: &mut [SolKeyedAccount], _info: &SolClusterInfo, _data: &[u8]) -> bool {
    let header = vec![1u8; 6];
    let option_a = vec![1u8; 1];
    let option_b = vec![1u8; 1];
    let init_poll = InitPollData {
        timeout: 10u32,
        header_len: 6,
        header: &header,
        option_a_len: 1,
        option_a: &option_a,
        option_b_len: 1,
        option_b: &option_b,
    };

    let key1 = [1u8; 32];
    let key2 = [1u8; 32];
    let key3 = [1u8; 32];
    let poll_data = PollData::init(init_poll, &key1, &key2, &key3, 5000);
    poll_data.to_bytes();

    info!("Success");
    true
}
