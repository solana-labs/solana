//! @brief SHA256 Syscall test

extern crate solana_program;
use solana_program::{
    custom_panic_default,
    hash::{hashv, Hasher},
    msg,
};

fn test_hasher() {
    let vals = &["Gaggablaghblagh!".as_ref(), "flurbos".as_ref()];
    let mut hasher = Hasher::default();
    hasher.hashv(vals);
    assert_eq!(hashv(vals), hasher.result());
}

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    msg!("sha256");

    test_hasher();

    0
}

custom_panic_default!();

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sha256() {
        test_hasher();
    }
}
