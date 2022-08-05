//! Hashing with the [SHA-256] hash function, and a general [`Hash`] type.
//!
//! [SHA-256]: https://en.wikipedia.org/wiki/SHA-2
//! [`Hash`]: struct@Hash

use {
    crate::{sanitize::Sanitize, wasm_bindgen},
    borsh::{BorshDeserialize, BorshSchema, BorshSerialize},
    generic_array::{typenum::U64, GenericArray},
    sha2::{compress256, Digest, Sha256},
    std::{convert::TryFrom, fmt, mem, str::FromStr},
    thiserror::Error,
};

/// Size of a hash in bytes.
pub const HASH_BYTES: usize = 32;
/// Maximum string length of a base58 encoded hash.
const MAX_BASE58_LEN: usize = 44;

/// Initial Value for SHA256 State
/// copied from https://github.com/RustCrypto/hashes/blob/457061f19b3538651aa7ef208ce6b04bfba65e61/sha2/src/consts.rs#L84
pub const H256_256: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// A hash; the 32-byte output of a hashing algorithm.
///
/// This struct is used most often in `solana-sdk` and related crates to contain
/// a [SHA-256] hash, but may instead contain a [blake3] hash, as created by the
/// [`blake3`] module (and used in [`Message::hash`]).
///
/// [SHA-256]: https://en.wikipedia.org/wiki/SHA-2
/// [blake3]: https://github.com/BLAKE3-team/BLAKE3
/// [`blake3`]: crate::blake3
/// [`Message::hash`]: crate::message::Message::hash
#[wasm_bindgen]
#[derive(
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Clone,
    Copy,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    AbiExample,
)]
#[repr(transparent)]
pub struct Hash(pub(crate) [u8; HASH_BYTES]);

#[derive(Clone, Default)]
pub struct Hasher {
    hasher: Sha256,
}

impl Hasher {
    pub fn hash(&mut self, val: &[u8]) {
        self.hasher.update(val);
    }
    pub fn hashv(&mut self, vals: &[&[u8]]) {
        for val in vals {
            self.hash(val);
        }
    }
    pub fn result(self) -> Hash {
        // At the time of this writing, the sha2 library is stuck on an old version
        // of generic_array (0.9.0). Decouple ourselves with a clone to our version.
        Hash(<[u8; HASH_BYTES]>::try_from(self.hasher.finalize().as_slice()).unwrap())
    }
}

impl Sanitize for Hash {}

impl From<[u8; HASH_BYTES]> for Hash {
    fn from(from: [u8; 32]) -> Self {
        Self(from)
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseHashError {
    #[error("string decoded to wrong size for hash")]
    WrongSize,
    #[error("failed to decoded string to hash")]
    Invalid,
}

impl FromStr for Hash {
    type Err = ParseHashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() > MAX_BASE58_LEN {
            return Err(ParseHashError::WrongSize);
        }
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| ParseHashError::Invalid)?;
        if bytes.len() != mem::size_of::<Hash>() {
            Err(ParseHashError::WrongSize)
        } else {
            Ok(Hash::new(&bytes))
        }
    }
}

impl Hash {
    pub fn new(hash_slice: &[u8]) -> Self {
        Hash(<[u8; HASH_BYTES]>::try_from(hash_slice).unwrap())
    }

    pub const fn new_from_array(hash_array: [u8; HASH_BYTES]) -> Self {
        Self(hash_array)
    }

    /// unique Hash for tests and benchmarks.
    pub fn new_unique() -> Self {
        use crate::atomic_u64::AtomicU64;
        static I: AtomicU64 = AtomicU64::new(1);

        let mut b = [0u8; HASH_BYTES];
        let i = I.fetch_add(1);
        b[0..8].copy_from_slice(&i.to_le_bytes());
        Self::new(&b)
    }

    pub fn to_bytes(self) -> [u8; HASH_BYTES] {
        self.0
    }
}

/// Return a Sha256 hash for the given data.
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
            crate::syscalls::sol_sha256(
                vals as *const _ as *const u8,
                vals.len() as u64,
                &mut hash_result as *mut _ as *mut u8,
            );
        }
        Hash::new_from_array(hash_result)
    }
}

/// Return a Sha256 hash for the given data.
pub fn hash(val: &[u8]) -> Hash {
    hashv(&[val])
}

/// Return the hash of the given hash extended with the given value.
pub fn extend_and_hash(id: &Hash, val: &[u8]) -> Hash {
    let mut hash_data = id.as_ref().to_vec();
    hash_data.extend_from_slice(val);
    hash(&hash_data)
}

/// Execute the compression function used internally by sha256 over 512-bit blocks, without the padding step.
/// This is not the same thing as the sha256 hash - chances are you want to use the `Hasher` instead.
/// IMPORTANT: The padding step of SHA256 is included in the standard for a reason - only use this if you know what you're doing.
pub fn compress(blocks: &[[u8; 64]]) -> Hash {
    let mut res = [0; HASH_BYTES];
    if blocks.len() == 0 {
        for (res_chunk, v) in res.chunks_exact_mut(4).zip(H256_256.iter()) {
            res_chunk.copy_from_slice(&v.to_be_bytes());
        }
        return Hash::new_from_array(res);
    }

    #[cfg(not(target_os = "solana"))]
    {
        // SAFETY: GenericArray<u8, U64> has the same memory layout as [u8; 64]
        let p = blocks.as_ptr() as *const GenericArray<u8, U64>;
        let blocks = unsafe { core::slice::from_raw_parts(p, blocks.len()) };

        let mut state = H256_256;
        compress256(&mut state, blocks);

        for (res_chunk, v) in res.chunks_exact_mut(4).zip(state.iter()) {
            res_chunk.copy_from_slice(&v.to_be_bytes());
        }

        Hash::new_from_array(res)
    }

    #[cfg(target_os = "solana")]
    {
        unsafe {
            crate::syscalls::sol_sha256_compress(
                blocks as *const _ as *const u8,
                blocks.len() as u64,
                &mut res as &mut _ as *mut u8,
            );
        }
        Hash::new_from_array(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_unique() {
        assert!(Hash::new_unique() != Hash::new_unique());
    }

    #[test]
    fn test_hash_fromstr() {
        let hash = hash(&[1u8]);

        let mut hash_base58_str = bs58::encode(hash).into_string();

        assert_eq!(hash_base58_str.parse::<Hash>(), Ok(hash));

        hash_base58_str.push_str(&bs58::encode(hash.0).into_string());
        assert_eq!(
            hash_base58_str.parse::<Hash>(),
            Err(ParseHashError::WrongSize)
        );

        hash_base58_str.truncate(hash_base58_str.len() / 2);
        assert_eq!(hash_base58_str.parse::<Hash>(), Ok(hash));

        hash_base58_str.truncate(hash_base58_str.len() / 2);
        assert_eq!(
            hash_base58_str.parse::<Hash>(),
            Err(ParseHashError::WrongSize)
        );

        let input_too_big = bs58::encode(&[0xffu8; HASH_BYTES + 1]).into_string();
        assert!(input_too_big.len() > MAX_BASE58_LEN);
        assert_eq!(
            input_too_big.parse::<Hash>(),
            Err(ParseHashError::WrongSize)
        );

        let mut hash_base58_str = bs58::encode(hash.0).into_string();
        assert_eq!(hash_base58_str.parse::<Hash>(), Ok(hash));

        // throw some non-base58 stuff in there
        hash_base58_str.replace_range(..1, "I");
        assert_eq!(
            hash_base58_str.parse::<Hash>(),
            Err(ParseHashError::Invalid)
        );
    }
}
