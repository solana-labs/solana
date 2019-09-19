//! @brief Example Rust-based BPF program tests dependent crates

extern crate solana_sdk;
use byteorder::{ByteOrder, LittleEndian};
use solana_sdk::entrypoint::SUCCESS;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
    let mut buf = [0; 4];
    LittleEndian::write_u32(&mut buf, 1_000_000);
    assert_eq!(1_000_000, LittleEndian::read_u32(&buf));

    let mut buf = [0; 2];
    LittleEndian::write_i16(&mut buf, -5_000);
    assert_eq!(-5_000, LittleEndian::read_i16(&buf));

    SUCCESS
}

#[cfg(test)]
mod test {
    use super::*;
    // Pulls in the stubs requried for `info!()`
    solana_sdk_bpf_test::stubs!();

    #[test]
    fn test_entrypoint() {
        assert_eq!(SUCCESS, entrypoint(std::ptr::null_mut()));
    }
}
