//! @brief Example Rust-based BPF program tests dependent crates

extern crate solana_sdk_bpf_utils;
use byteorder::{ByteOrder, LittleEndian};
<<<<<<< HEAD
use solana_sdk_bpf_utils::info;
=======
use solana_sdk::entrypoint::SUCCESS;
use solana_sdk::info;
>>>>>>> 81c36699c...  Add support for BPF program custom errors (#5743)

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
    let mut buf = [0; 4];
    LittleEndian::write_u32(&mut buf, 1_000_000);
    assert_eq!(1_000_000, LittleEndian::read_u32(&buf));

    let mut buf = [0; 2];
    LittleEndian::write_i16(&mut buf, -5_000);
    assert_eq!(-5_000, LittleEndian::read_i16(&buf));

    info!("Success");
    SUCCESS
}
