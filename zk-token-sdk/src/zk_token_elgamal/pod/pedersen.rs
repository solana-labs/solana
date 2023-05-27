use {
    crate::zk_token_elgamal::pod::{Pod, Zeroable},
    std::fmt,
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{encryption::pedersen as decoded, errors::ProofError},
    curve25519_dalek::ristretto::CompressedRistretto,
};

#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct PedersenCommitment(pub [u8; 32]);

impl fmt::Debug for PedersenCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::PedersenCommitment> for PedersenCommitment {
    fn from(comm: decoded::PedersenCommitment) -> Self {
        Self(comm.to_bytes())
    }
}

// For proof verification, interpret pod::PedersenComm directly as CompressedRistretto
#[cfg(not(target_os = "solana"))]
impl From<PedersenCommitment> for CompressedRistretto {
    fn from(pod: PedersenCommitment) -> Self {
        Self(pod.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PedersenCommitment> for decoded::PedersenCommitment {
    type Error = ProofError;

    fn try_from(pod: PedersenCommitment) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0).ok_or(ProofError::CiphertextDeserialization)
    }
}
