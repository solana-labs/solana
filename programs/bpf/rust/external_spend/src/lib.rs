//! @brief Example Rust-based BPF program that moves a lamport from one account to another

#![no_std]
#![allow(unreachable_code)]
#![allow(unused_attributes)]

extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::entrypoint::*;
use solana_sdk_bpf_utils::{entrypoint, info};

entrypoint!(process_instruction);
fn process_instruction(ka: &mut [SolKeyedAccount], _info: &SolClusterInfo, _data: &[u8]) -> bool {
    // account 0 is the mint and not owned by this program, any debit of its lamports
    // should result in a failed program execution.  Test to ensure that this debit
    // is seen by the runtime and fails as expected
    *ka[0].lamports -= 1;

    info!("Success");
    true
}
