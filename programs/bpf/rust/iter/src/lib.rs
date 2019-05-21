//! @brief Example Rust-based BPF program that prints out the parameters passed to it

#![no_std]

extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::log::*;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    const ITERS: usize = 100;
    let ones = [1_u64; ITERS];
    let mut sum: u64 = 0;

    for v in ones.iter() {
        sum += *v;
    }
    sol_log_64(0xff, 0, 0, 0, sum);
    assert_eq!(sum, ITERS as u64);

    sol_log("Success");
    true
}
