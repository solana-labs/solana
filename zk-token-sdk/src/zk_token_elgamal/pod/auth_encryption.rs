//! Plain Old Data types for the AES128-GCM-SIV authenticated encryption scheme.

use serde::{
    de::{self, SeqAccess, Visitor},
    Deserialize, Deserializer,
};

#[cfg(not(target_os = "solana"))]
use crate::encryption::auth_encryption::{self as decoded, AuthenticatedEncryptionError};
use {
    crate::zk_token_elgamal::pod::{Pod, Zeroable},
    base64::{prelude::BASE64_STANDARD, Engine},
    std::fmt,
};

/// Byte length of an authenticated encryption ciphertext
const AE_CIPHERTEXT_LEN: usize = 36;

struct AeCiphertextVisitor;

/// The `AeCiphertext` type as a `Pod`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct AeCiphertext(pub [u8; AE_CIPHERTEXT_LEN]);

// `AeCiphertext` is a wrapper type for a byte array, which is both `Pod` and `Zeroable`. However,
// the marker traits `bytemuck::Pod` and `bytemuck::Zeroable` can only be derived for power-of-two
// length byte arrays. Directly implement these traits for `AeCiphertext`.
unsafe impl Zeroable for AeCiphertext {}
unsafe impl Pod for AeCiphertext {}

impl fmt::Debug for AeCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl fmt::Display for AeCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl Default for AeCiphertext {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl<'de> Visitor<'de> for AeCiphertextVisitor {
    type Value = AeCiphertext;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a byte array of length AE_CIPHERTEXT_LEN")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut arr = [0u8; AE_CIPHERTEXT_LEN];
        for i in 0..AE_CIPHERTEXT_LEN {
            arr[i] = seq
                .next_element()?
                .ok_or(de::Error::invalid_length(i, &self))?;
        }
        Ok(AeCiphertext(arr))
    }
}

impl<'de> Deserialize<'de> for AeCiphertext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(AE_CIPHERTEXT_LEN, AeCiphertextVisitor)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::AeCiphertext> for AeCiphertext {
    fn from(decoded_ciphertext: decoded::AeCiphertext) -> Self {
        Self(decoded_ciphertext.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<AeCiphertext> for decoded::AeCiphertext {
    type Error = AuthenticatedEncryptionError;

    fn try_from(pod_ciphertext: AeCiphertext) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_ciphertext.0).ok_or(AuthenticatedEncryptionError::Deserialization)
    }
}
