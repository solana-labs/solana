//! SHA Syscall test

extern crate solana_program;
use solana_program::{custom_heap_default, custom_panic_default, msg};

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

fn test_blake3_hasher() {
    use solana_program::blake3::hashv;
    let v0: &[u8] = b"Gaggablaghblagh!";
    let v1: &[u8] = b"flurbos!";
    let vals: &[&[u8]] = &[v0, v1];
    let hash = blake3::hash(&[v0, v1].concat());
    assert_eq!(hashv(vals).0, *hash.as_bytes());
}

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    msg!("sha");

    test_sha256_hasher();
    test_keccak256_hasher();
    test_blake3_hasher();

    0
}

custom_heap_default!();
custom_panic_default!();

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sha() {
        test_sha256_hasher();
        test_keccak256_hasher();
        test_blake3_hasher();
    }
}
