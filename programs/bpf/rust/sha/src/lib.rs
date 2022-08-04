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

fn test_sha256_compression() {
    use solana_program::hash::{compress, H256_256, Hasher};
    use generic_array::{GenericArray, typenum::U64};

    let vals = &["Gaggablaghblagh!".as_ref(), "flurbos".as_ref()];
    let mut hasher = Hasher::default();
    hasher.hashv(vals);
    let left = hasher.result();

    let vals = &["wubbbawubbadubdub!".as_ref(), "dinglebop".as_ref()];
    let mut hasher = Hasher::default();
    hasher.hashv(vals);
    let right = hasher.result();

    let mut block = [0u8; 64];
    (&mut block[..32]).copy_from_slice(&left.to_bytes());
    (&mut block[32..]).copy_from_slice(&right.to_bytes());
    let compressed = compress(&[block]);

    let mut state = H256_256;
    let block_arr = GenericArray::<u8, U64>::from(block);
    sha2::compress256(&mut state, &[block_arr]);

    let mut compressed_correct = [0u8; 32];
    for (chunk, v) in compressed_correct.chunks_exact_mut(4).zip(state.iter()) {
        chunk.copy_from_slice(&v.to_be_bytes());
    }

    assert_eq!(compressed.to_bytes(), compressed_correct);
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
