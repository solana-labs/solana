//! @brief Example Rust-based BPF program that panics

extern crate solana_program;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    panic!();
}
