//! Hashing with the [keccak] (SHA-3) hash function.
//!
//! [keccak]: https://keccak.team/keccak.html
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(feature = "frozen-abi", feature(min_specialization))]
#![no_std]
#[cfg(feature = "std")]
extern crate std;

#[cfg(any(feature = "sha3", not(target_os = "solana")))]
use sha3::{Digest, Keccak256};
pub use solana_hash::{ParseHashError, HASH_BYTES, MAX_BASE58_LEN};
#[cfg(feature = "borsh")]
use {
    borsh::{BorshDeserialize, BorshSchema, BorshSerialize},
    std::string::ToString,
};
use {
    core::{fmt, str::FromStr},
    solana_sanitize::Sanitize,
};

// TODO: replace this with `solana_hash::Hash` in the
// next breaking change.
// It's a breaking change because the field is public
// here and private in `solana_hash`, and making
// it public in `solana_hash` would break wasm-bindgen
#[cfg_attr(feature = "frozen-abi", derive(solana_frozen_abi_macro::AbiExample))]
#[cfg_attr(
    feature = "borsh",
    derive(BorshSerialize, BorshDeserialize, BorshSchema),
    borsh(crate = "borsh")
)]
#[cfg_attr(
    feature = "serde",
    derive(serde_derive::Deserialize, serde_derive::Serialize)
)]
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct Hash(pub [u8; HASH_BYTES]);

#[cfg(any(feature = "sha3", not(target_os = "solana")))]
#[derive(Clone, Default)]
pub struct Hasher {
    hasher: Keccak256,
}

#[cfg(any(feature = "sha3", not(target_os = "solana")))]
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
        Hash(self.hasher.finalize().into())
    }
}

impl From<solana_hash::Hash> for Hash {
    fn from(val: solana_hash::Hash) -> Self {
        Self(val.to_bytes())
    }
}

impl From<Hash> for solana_hash::Hash {
    fn from(val: Hash) -> Self {
        Self::new_from_array(val.0)
    }
}

impl Sanitize for Hash {}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let converted: solana_hash::Hash = (*self).into();
        fmt::Debug::fmt(&converted, f)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let converted: solana_hash::Hash = (*self).into();
        fmt::Display::fmt(&converted, f)
    }
}

impl FromStr for Hash {
    type Err = ParseHashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let unconverted = solana_hash::Hash::from_str(s)?;
        Ok(unconverted.into())
    }
}

impl Hash {
    #[deprecated(since = "2.2.0", note = "Use 'Hash::new_from_array' instead")]
    pub fn new(hash_slice: &[u8]) -> Self {
        #[allow(deprecated)]
        Self::from(solana_hash::Hash::new(hash_slice))
    }

    pub const fn new_from_array(hash_array: [u8; HASH_BYTES]) -> Self {
        Self(hash_array)
    }

    /// unique Hash for tests and benchmarks.
    pub fn new_unique() -> Self {
        Self::from(solana_hash::Hash::new_unique())
    }

    pub fn to_bytes(self) -> [u8; HASH_BYTES] {
        self.0
    }
}

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
            solana_define_syscall::definitions::sol_keccak256(
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

#[cfg(feature = "std")]
/// Return the hash of the given hash extended with the given value.
pub fn extend_and_hash(id: &Hash, val: &[u8]) -> Hash {
    let mut hash_data = id.as_ref().to_vec();
    hash_data.extend_from_slice(val);
    hash(&hash_data)
}
