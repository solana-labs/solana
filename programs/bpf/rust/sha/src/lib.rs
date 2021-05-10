//! @brief SHA Syscall test

extern crate solana_program;
use solana_program::{custom_panic_default, msg};

fn test_sha256_hasher() {
    use solana_program::hash::{hashv, Hasher};
    let vals = &["Gaggablaghblagh!".as_ref(), "flurbos".as_ref()];
    let mut hasher = Hasher::default();
    hasher.hashv(vals);
    assert_eq!(hashv(vals), hasher.result());
}

fn test_keccak256_hasher() {
    use solana_program::keccak::{hashv, Hasher};
    let vals = &["Gaggablaghblagh!".as_ref(), "flurbos".as_ref()];
    let mut hasher = Hasher::default();
    hasher.hashv(vals);
    assert_eq!(hashv(vals), hasher.result());
}

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    msg!("sha");

    test_sha256_hasher();
    test_keccak256_hasher();

    0
}

custom_panic_default!();

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sha() {
        test_sha256_hasher();
        test_keccak256_hasher();
    }
}
