//! @brief SHA256 Syscall test

extern crate solana_sdk;
use solana_sdk::{
    hash::{hashv, Hasher},
    info,
};

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    info!("sha256");

    let vals = &["Gaggablaghblagh!".as_ref(), "flurbos".as_ref()];
    let mut hasher = Hasher::default();
    hasher.hashv(vals);
    assert_eq!(hashv(vals), hasher.result());

    0
}
