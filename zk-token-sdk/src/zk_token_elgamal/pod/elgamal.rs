use {
    crate::zk_token_elgamal::pod::{Pod, Zeroable},
    base64::{prelude::BASE64_STANDARD, Engine},
    std::fmt,
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{encryption::elgamal as decoded, errors::ProofError},
    curve25519_dalek::ristretto::CompressedRistretto,
};

#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct ElGamalCiphertext(pub [u8; 64]);

impl fmt::Debug for ElGamalCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl fmt::Display for ElGamalCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl Default for ElGamalCiphertext {
    fn default() -> Self {
        Self::zeroed()
    }
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::ElGamalCiphertext> for ElGamalCiphertext {
    fn from(ct: decoded::ElGamalCiphertext) -> Self {
        Self(ct.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ElGamalCiphertext> for decoded::ElGamalCiphertext {
    type Error = ProofError;

    fn try_from(ct: ElGamalCiphertext) -> Result<Self, Self::Error> {
        Self::from_bytes(&ct.0).ok_or(ProofError::CiphertextDeserialization)
    }
}

#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct ElGamalPubkey(pub [u8; 32]);

impl fmt::Debug for ElGamalPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl fmt::Display for ElGamalPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::ElGamalPubkey> for ElGamalPubkey {
    fn from(pk: decoded::ElGamalPubkey) -> Self {
        Self(pk.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ElGamalPubkey> for decoded::ElGamalPubkey {
    type Error = ProofError;

    fn try_from(pk: ElGamalPubkey) -> Result<Self, Self::Error> {
        Self::from_bytes(&pk.0).ok_or(ProofError::CiphertextDeserialization)
    }
}

#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct DecryptHandle(pub [u8; 32]);

impl fmt::Debug for DecryptHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::DecryptHandle> for DecryptHandle {
    fn from(handle: decoded::DecryptHandle) -> Self {
        Self(handle.to_bytes())
    }
}

// For proof verification, interpret pod::PedersenDecHandle as CompressedRistretto
#[cfg(not(target_os = "solana"))]
impl From<DecryptHandle> for CompressedRistretto {
    fn from(pod: DecryptHandle) -> Self {
        Self(pod.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<DecryptHandle> for decoded::DecryptHandle {
    type Error = ProofError;

    fn try_from(pod: DecryptHandle) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0).ok_or(ProofError::CiphertextDeserialization)
    }
}
