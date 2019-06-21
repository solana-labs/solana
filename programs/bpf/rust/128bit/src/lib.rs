//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::info;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    let x: u128 = 1;
    let y = x.rotate_right(1);
    assert_eq!(y, 170141183460469231731687303715884105728);
    
    assert_eq!(u128::max_value(), 340282366920938463463374607431768211455);

    let mut z = u128::max_value();
    z -= 1;
    assert_eq!(z, 340282366920938463463374607431768211454);

    let x = u64::max_value();
    let y = u64::max_value();
    assert_eq!(x as u128 + y as u128, 36893488147419103230);

    let x = solana_bpf_rust_128bit_dep::work(u64::max_value() as u128, u64::max_value() as u128);
    assert_eq!(x.wrapping_shr(64) as u64, 1);
    assert_eq!(x.wrapping_shl(64).wrapping_shr(64) as u64, 0xfffffffffffffffe);
    assert_eq!(x, 0x1fffffffffffffffe);

    info!("Success");
    true
}
