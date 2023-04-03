//! Hashing with the [keccak] (SHA-3) hash function.
//!
//! [keccak]: https://keccak.team/keccak.html

pub use crate::hash::{ParseHashError, HASH_BYTES};
use {
    crate::hash::{self, Hash},
    sha3::Keccak256,
};

/// A Keccak256 hasher.
pub type Hasher = hash::GenericHasher<Keccak256>;

/// Return a Keccak256 hash for the given data.
pub fn hashv(vals: &[&[u8]]) -> Hash {
    // Perform the calculation inline, calling this from within a program is
    // not supported
    #[cfg(not(target_os = "solana"))]
    {
        let mut hasher = Hasher::default();
        hasher.hashv(vals);
        hasher.result()
    }
    // Call via a system call to perform the calculation
    #[cfg(target_os = "solana")]
    {
        let mut hash_result = [0; HASH_BYTES];
        unsafe {
            crate::syscalls::sol_keccak256(
                vals as *const _ as *const u8,
                vals.len() as u64,
                &mut hash_result as *mut _ as *mut u8,
            );
        }
        Hash::new_from_array(hash_result)
    }
}

/// Return a Keccak256 hash for the given data.
pub fn hash(val: &[u8]) -> Hash {
    hashv(&[val])
}

/// Return the hash of the given hash extended with the given value.
pub fn extend_and_hash(id: &Hash, val: &[u8]) -> Hash {
    let mut hash_data = id.as_ref().to_vec();
    hash_data.extend_from_slice(val);
    hash(&hash_data)
}
