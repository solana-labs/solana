use {
    crate::{
        curve25519::ristretto::PodRistrettoPoint,
        encryption::elgamal,
        errors::ProofError,
        pod::{pedersen::PedersenCommitment, Pod, Zeroable},
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    curve25519_dalek::ristretto::CompressedRistretto,
    std::fmt,
};

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
impl From<elgamal::ElGamalPubkey> for ElGamalPubkey {
    fn from(pk: elgamal::ElGamalPubkey) -> Self {
        Self(pk.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ElGamalPubkey> for elgamal::ElGamalPubkey {
    type Error = ProofError;

    fn try_from(pk: ElGamalPubkey) -> Result<Self, Self::Error> {
        Self::from_bytes(&pk.0).ok_or(ProofError::CiphertextDeserialization)
    }
}

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
impl From<elgamal::ElGamalCiphertext> for ElGamalCiphertext {
    fn from(ct: elgamal::ElGamalCiphertext) -> Self {
        Self(ct.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ElGamalCiphertext> for elgamal::ElGamalCiphertext {
    type Error = ProofError;

    fn try_from(ct: ElGamalCiphertext) -> Result<Self, Self::Error> {
        Self::from_bytes(&ct.0).ok_or(ProofError::CiphertextDeserialization)
    }
}

impl From<(PedersenCommitment, DecryptHandle)> for ElGamalCiphertext {
    fn from((commitment, handle): (PedersenCommitment, DecryptHandle)) -> Self {
        let mut buf = [0_u8; 64];
        buf[..32].copy_from_slice(&commitment.0);
        buf[32..].copy_from_slice(&handle.0);
        ElGamalCiphertext(buf)
    }
}

impl From<ElGamalCiphertext> for (PedersenCommitment, DecryptHandle) {
    fn from(ciphertext: ElGamalCiphertext) -> Self {
        let commitment: [u8; 32] = ciphertext.0[..32].try_into().unwrap();
        let handle: [u8; 32] = ciphertext.0[32..].try_into().unwrap();

        (PedersenCommitment(commitment), DecryptHandle(handle))
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
impl From<elgamal::DecryptHandle> for DecryptHandle {
    fn from(handle: elgamal::DecryptHandle) -> Self {
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
impl TryFrom<DecryptHandle> for elgamal::DecryptHandle {
    type Error = ProofError;

    fn try_from(pod: DecryptHandle) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0).ok_or(ProofError::CiphertextDeserialization)
    }
}

impl From<DecryptHandle> for PodRistrettoPoint {
    fn from(handle: DecryptHandle) -> Self {
        PodRistrettoPoint(handle.0)
    }
}

impl From<PodRistrettoPoint> for DecryptHandle {
    fn from(point: PodRistrettoPoint) -> Self {
        DecryptHandle(point.0)
    }
}
