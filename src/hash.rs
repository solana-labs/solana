//! The `hash` module provides functions for creating SHA-256 hashes.

use bs58;
use generic_array::typenum::U32;
use generic_array::GenericArray;
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Hash(GenericArray<u8, U32>);

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}
/// Return a Sha256 hash for the given data.
pub fn hash(val: &[u8]) -> Hash {
    let mut hasher = Sha256::default();
    hasher.input(val);

    // At the time of this writing, the sha2 library is stuck on an old version
    // of generic_array (0.9.0). Decouple ourselves with a clone to our version.
    Hash(GenericArray::clone_from_slice(hasher.result().as_slice()))
}

/// Return the hash of the given hash extended with the given value.
pub fn extend_and_hash(id: &Hash, val: &[u8]) -> Hash {
    let mut hash_data = id.as_ref().to_vec();
    hash_data.extend_from_slice(val);
    hash(&hash_data)
}
