#![no_std]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#[cfg(feature = "borsh")]
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
#[cfg(any(feature = "std", target_arch = "wasm32"))]
extern crate std;
#[cfg(feature = "bytemuck")]
use bytemuck_derive::{Pod, Zeroable};
#[cfg(feature = "serde")]
use serde_derive::{Deserialize, Serialize};
#[cfg(any(all(feature = "borsh", feature = "std"), target_arch = "wasm32"))]
use std::string::ToString;
use {
    core::{
        convert::TryFrom,
        fmt, mem,
        str::{from_utf8, FromStr},
    },
    solana_sanitize::Sanitize,
};
#[cfg(target_arch = "wasm32")]
use {
    js_sys::{Array, Uint8Array},
    std::{boxed::Box, format, string::String, vec},
    wasm_bindgen::{prelude::*, JsCast},
};

/// Size of a hash in bytes.
pub const HASH_BYTES: usize = 32;
/// Maximum string length of a base58 encoded hash.
pub const MAX_BASE58_LEN: usize = 44;

/// A hash; the 32-byte output of a hashing algorithm.
///
/// This struct is used most often in `solana-sdk` and related crates to contain
/// a [SHA-256] hash, but may instead contain a [blake3] hash.
///
/// [SHA-256]: https://en.wikipedia.org/wiki/SHA-2
/// [blake3]: https://github.com/BLAKE3-team/BLAKE3
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[cfg_attr(feature = "frozen-abi", derive(solana_frozen_abi_macro::AbiExample))]
#[cfg_attr(
    feature = "borsh",
    derive(BorshSerialize, BorshDeserialize),
    borsh(crate = "borsh")
)]
#[cfg_attr(all(feature = "borsh", feature = "std"), derive(BorshSchema))]
#[cfg_attr(feature = "bytemuck", derive(Pod, Zeroable))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize,))]
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct Hash(pub(crate) [u8; HASH_BYTES]);

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

fn write_as_base58(f: &mut fmt::Formatter, h: &Hash) -> fmt::Result {
    let mut out = [0u8; MAX_BASE58_LEN];
    let out_slice: &mut [u8] = &mut out;
    // This will never fail because the only possible error is BufferTooSmall,
    // and we will never call it with too small a buffer.
    let len = bs58::encode(h.0).onto(out_slice).unwrap();
    let as_str = from_utf8(&out[..len]).unwrap();
    f.write_str(as_str)
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write_as_base58(f, self)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write_as_base58(f, self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseHashError {
    WrongSize,
    Invalid,
}

#[cfg(feature = "std")]
impl std::error::Error for ParseHashError {}

impl fmt::Display for ParseHashError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseHashError::WrongSize => f.write_str("string decoded to wrong size for hash"),
            ParseHashError::Invalid => f.write_str("failed to decoded string to hash"),
        }
    }
}

impl FromStr for Hash {
    type Err = ParseHashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() > MAX_BASE58_LEN {
            return Err(ParseHashError::WrongSize);
        }
        let mut bytes = [0; HASH_BYTES];
        let decoded_size = bs58::decode(s)
            .onto(&mut bytes)
            .map_err(|_| ParseHashError::Invalid)?;
        if decoded_size != mem::size_of::<Hash>() {
            Err(ParseHashError::WrongSize)
        } else {
            Ok(bytes.into())
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
        use solana_atomic_u64::AtomicU64;
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

#[cfg(target_arch = "wasm32")]
#[allow(non_snake_case)]
#[wasm_bindgen]
impl Hash {
    /// Create a new Hash object
    ///
    /// * `value` - optional hash as a base58 encoded string, `Uint8Array`, `[number]`
    #[wasm_bindgen(constructor)]
    pub fn constructor(value: JsValue) -> Result<Hash, JsValue> {
        if let Some(base58_str) = value.as_string() {
            base58_str
                .parse::<Hash>()
                .map_err(|x| JsValue::from(x.to_string()))
        } else if let Some(uint8_array) = value.dyn_ref::<Uint8Array>() {
            Ok(Hash::new(&uint8_array.to_vec()))
        } else if let Some(array) = value.dyn_ref::<Array>() {
            let mut bytes = vec![];
            let iterator = js_sys::try_iter(&array.values())?.expect("array to be iterable");
            for x in iterator {
                let x = x?;

                if let Some(n) = x.as_f64() {
                    if n >= 0. && n <= 255. {
                        bytes.push(n as u8);
                        continue;
                    }
                }
                return Err(format!("Invalid array argument: {:?}", x).into());
            }
            Ok(Hash::new(&bytes))
        } else if value.is_undefined() {
            Ok(Hash::default())
        } else {
            Err("Unsupported argument".into())
        }
    }

    /// Return the base58 string representation of the hash
    pub fn toString(&self) -> String {
        self.to_string()
    }

    /// Checks if two `Hash`s are equal
    pub fn equals(&self, other: &Hash) -> bool {
        self == other
    }

    /// Return the `Uint8Array` representation of the hash
    pub fn toBytes(&self) -> Box<[u8]> {
        self.0.clone().into()
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
        let hash = Hash::new_from_array([1; 32]);

        let mut hash_base58_str = bs58::encode(hash).into_string();

        assert_eq!(hash_base58_str.parse::<Hash>(), Ok(hash));

        hash_base58_str.push_str(&bs58::encode(hash.as_ref()).into_string());
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

        let mut hash_base58_str = bs58::encode(hash.as_ref()).into_string();
        assert_eq!(hash_base58_str.parse::<Hash>(), Ok(hash));

        // throw some non-base58 stuff in there
        hash_base58_str.replace_range(..1, "I");
        assert_eq!(
            hash_base58_str.parse::<Hash>(),
            Err(ParseHashError::Invalid)
        );
    }
}
