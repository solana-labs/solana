//! @brief Example Rust-based BPF program that panics

extern crate solana_sdk;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
    panic!();
}
