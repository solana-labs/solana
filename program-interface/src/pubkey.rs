use crate::decode_error::DecodeError;
use num_derive::{FromPrimitive, ToPrimitive};
use std::{convert::TryFrom, fmt, mem, str::FromStr};
use thiserror::Error;

pub use bs58;

/// maximum length of derived pubkey seed
pub const MAX_SEED_LEN: usize = 32;

#[derive(Error, Debug, Serialize, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum PubkeyError {
    #[error("length of requested seed is too long")]
    MaxSeedLengthExceeded,
}
impl<T> DecodeError<T> for PubkeyError {
    fn type_of() -> &'static str {
        "PubkeyError"
    }
}

#[repr(transparent)]
#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pubkey([u8; 32]);

impl crate::sanitize::Sanitize for Pubkey {}

#[derive(Error, Debug, Serialize, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum ParsePubkeyError {
    #[error("String is the wrong size")]
    WrongSize,
    #[error("Invalid Base58 string")]
    Invalid,
}
impl<T> DecodeError<T> for ParsePubkeyError {
    fn type_of() -> &'static str {
        "ParsePubkeyError"
    }
}

impl FromStr for Pubkey {
    type Err = ParsePubkeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pubkey_vec = bs58::decode(s)
            .into_vec()
            .map_err(|_| ParsePubkeyError::Invalid)?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            Err(ParsePubkeyError::WrongSize)
        } else {
            Ok(Pubkey::new(&pubkey_vec))
        }
    }
}

impl Pubkey {
    pub fn new(pubkey_vec: &[u8]) -> Self {
        Self(
            <[u8; 32]>::try_from(<&[u8]>::clone(&pubkey_vec))
                .expect("Slice must be the same length as a Pubkey"),
        )
    }

    pub const fn new_from_array(pubkey_array: [u8; 32]) -> Self {
        Self(pubkey_array)
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl AsRef<[u8]> for Pubkey {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Debug for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

impl fmt::Display for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pubkey_fromstr() {
        let pubkey = Pubkey::new(&[
            11, 22, 33, 44, 55, 66, 77, 88, 99, 00, 11, 22, 33, 44, 55, 66, 77, 88, 99, 00, 11, 22,
            33, 44, 55, 66, 77, 88, 99, 00, 11, 22,
        ]);

        let mut pubkey_base58_str = bs58::encode(pubkey.0).into_string();

        assert_eq!(pubkey_base58_str.parse::<Pubkey>(), Ok(pubkey));

        pubkey_base58_str.push_str(&bs58::encode(pubkey.0).into_string());
        assert_eq!(
            pubkey_base58_str.parse::<Pubkey>(),
            Err(ParsePubkeyError::WrongSize)
        );

        pubkey_base58_str.truncate(pubkey_base58_str.len() / 2);
        assert_eq!(pubkey_base58_str.parse::<Pubkey>(), Ok(pubkey));

        pubkey_base58_str.truncate(pubkey_base58_str.len() / 2);
        assert_eq!(
            pubkey_base58_str.parse::<Pubkey>(),
            Err(ParsePubkeyError::WrongSize)
        );

        let mut pubkey_base58_str = bs58::encode(pubkey.0).into_string();
        assert_eq!(pubkey_base58_str.parse::<Pubkey>(), Ok(pubkey));

        // throw some non-base58 stuff in there
        pubkey_base58_str.replace_range(..1, "I");
        assert_eq!(
            pubkey_base58_str.parse::<Pubkey>(),
            Err(ParsePubkeyError::Invalid)
        );
    }
}
