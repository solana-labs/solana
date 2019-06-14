//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

mod helper;

extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::log::*;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    sol_log("call same package");
    assert_eq!(crate::helper::many_args(1, 2, 3, 4, 5, 6, 7, 8, 9), 45);
    sol_log("call another package");
    assert_eq!(
        solana_bpf_rust_many_args_dep::many_args(1, 2, 3, 4, 5, 6, 7, 8, 9),
        45
    );

    sol_log("Success");
    true
}
