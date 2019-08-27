//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

#[cfg(not(test))]
extern crate solana_sdk_bpf_no_std;
extern crate solana_sdk_bpf_utils;

use solana_bpf_rust_stack_bug_dep::{Data, TestDep};
// use solana_sdk_bpf_utils::info;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    let array = [0xA, 0xB, 0xC, 0xD, 0xE, 0xF];
    let data = Data {
        tone: 21u64,
        ttwo: 22u64,
        tthree: 23u64,
        tfour: 24u64,
        tfive: 25u32,
        array: &array,
    };

    let test_dep = TestDep::new(
        &data,
        1,
        2,
        3,
        4,
        5,
        6,
        7,
        8,
    );
    if test_dep.ten == 10 {
        true
    } else {
        false
    }
}
