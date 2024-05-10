//! Plain Old Data type for the Pedersen commitment scheme.

use {
    crate::encryption::PEDERSEN_COMMITMENT_LEN,
    bytemuck::{Pod, Zeroable},
    std::fmt,
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{encryption::pedersen::PedersenCommitment, errors::ElGamalError},
    curve25519_dalek::ristretto::CompressedRistretto,
};

/// The `PedersenCommitment` type as a `Pod`.
#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct PodPedersenCommitment(pub(crate) [u8; PEDERSEN_COMMITMENT_LEN]);

impl fmt::Debug for PodPedersenCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<PedersenCommitment> for PodPedersenCommitment {
    fn from(decoded_commitment: PedersenCommitment) -> Self {
        Self(decoded_commitment.to_bytes())
    }
}

// For proof verification, interpret pod::PedersenCommitment directly as CompressedRistretto
#[cfg(not(target_os = "solana"))]
impl From<PodPedersenCommitment> for CompressedRistretto {
    fn from(pod_commitment: PodPedersenCommitment) -> Self {
        Self(pod_commitment.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodPedersenCommitment> for PedersenCommitment {
    type Error = ElGamalError;

    fn try_from(pod_commitment: PodPedersenCommitment) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_commitment.0).ok_or(ElGamalError::CiphertextDeserialization)
    }
}
