//! @brief SHA256 Syscall test

extern crate solana_sdk;
use solana_sdk::{
    hash::{hashv, Hasher},
    info,
};

fn test_hasher() {
    let vals = &["Gaggablaghblagh!".as_ref(), "flurbos".as_ref()];
    let mut hasher = Hasher::default();
    hasher.hashv(vals);
    assert_eq!(hashv(vals), hasher.result());
}

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    info!("sha256");

    test_hasher();

    0
}

#[cfg(test)]
mod test {
    use super::*;
    // Pull in syscall stubs when building for non-BPF targets
    solana_sdk::program_stubs!();

    #[test]
    fn test_sha256() {
        test_hasher();
    }
}
