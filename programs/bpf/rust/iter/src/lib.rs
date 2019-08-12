//! @brief Example Rust-based BPF program tests loop iteration

// #![no_std]
#![allow(unused_attributes)]

// #[cfg(not(test))]
// extern crate solana_sdk_bpf_no_std;
extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::info;
use std::cmp;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    const ITERS: usize = 100;
    let ones = [1_u64; ITERS];
    let mut sum: u64 = 0;

    for v in ones.iter() {
        sum += *v;
    }
    info!(0xff, 0, 0, 0, sum);
    assert_eq!(sum, ITERS as u64);

    assert_eq!(100, cmp::max(100, sum));

    info!("info");
    println!("foo");

    info!("Success");
    true
}
