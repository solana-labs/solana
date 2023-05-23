use {
    crate::{
        encryption::auth_encryption,
        errors::ProofError,
        zk_token_elgamal::pod::{Pod, Zeroable},
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    std::fmt,
};

/// Serialization for AeCiphertext
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct AeCiphertext(pub [u8; 36]);

// `AeCiphertext` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
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
impl From<auth_encryption::AeCiphertext> for AeCiphertext {
    fn from(ct: auth_encryption::AeCiphertext) -> Self {
        Self(ct.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<AeCiphertext> for auth_encryption::AeCiphertext {
    type Error = ProofError;

    fn try_from(ct: AeCiphertext) -> Result<Self, Self::Error> {
        Self::from_bytes(&ct.0).ok_or(ProofError::CiphertextDeserialization)
    }
}
