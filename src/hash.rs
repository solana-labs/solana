//! The `hash` module provides functions for creating SHA-256 hashes.

use generic_array::typenum::U32;
use generic_array::GenericArray;
use sha2::{Digest, Sha256};

pub type Hash = GenericArray<u8, U32>;

/// Return a Sha256 hash for the given data.
pub fn hash(val: &[u8]) -> Hash {
    let mut hasher = Sha256::default();
    hasher.input(val);

    // At the time of this writing, the sha2 library is stuck on an old version
    // of generic_array (0.9.0). Decouple ourselves with a clone to our version.
    GenericArray::clone_from_slice(hasher.result().as_slice())
}

/// Return the hash of the given hash extended with the given value.
pub fn extend_and_hash(id: &Hash, val: &[u8]) -> Hash {
    let mut hash_data = id.to_vec();
    hash_data.extend_from_slice(val);
    hash(&hash_data)
}
