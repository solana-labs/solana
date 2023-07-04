//! Plain Old Data types for the AES128-GCM-SIV authenticated encryption scheme.

#[cfg(not(target_os = "solana"))]
use crate::{encryption::auth_encryption as decoded, errors::ProofError};
use {
    crate::zk_token_elgamal::pod::{Pod, Zeroable},
    base64::{prelude::BASE64_STANDARD, Engine},
    std::fmt,
};

/// Byte length of an authenticated encryption ciphertext
const AE_CIPHERTEXT_LEN: usize = 36;

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

#[cfg(not(target_os = "solana"))]
impl From<decoded::AeCiphertext> for AeCiphertext {
    fn from(decoded_ciphertext: decoded::AeCiphertext) -> Self {
        Self(decoded_ciphertext.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<AeCiphertext> for decoded::AeCiphertext {
    type Error = ProofError;

    fn try_from(pod_ciphertext: AeCiphertext) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_ciphertext.0).ok_or(ProofError::CiphertextDeserialization)
    }
}
