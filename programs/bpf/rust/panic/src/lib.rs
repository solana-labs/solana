//! @brief Example Rust-based BPF program that prints out the parameters passed to it

#![no_std]

extern crate solana_sdk_bpf_utils;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    panic!();
}
