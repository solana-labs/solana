//! @brief Example Rust-based BPF program tests dependent crates

#![no_std]
#![allow(unused_attributes)]

#[cfg(not(test))]
extern crate solana_sdk_bpf_no_std;
extern crate solana_sdk_bpf_utils;

use byteorder::{ByteOrder, LittleEndian};
use solana_sdk_bpf_utils::info;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    let mut buf = [0; 4];
    LittleEndian::write_u32(&mut buf, 1_000_000);
    assert_eq!(1_000_000, LittleEndian::read_u32(&buf));

    let mut buf = [0; 2];
    LittleEndian::write_i16(&mut buf, -5_000);
    assert_eq!(-5_000, LittleEndian::read_i16(&buf));

    info!("Success");
    true
}
