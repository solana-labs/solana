//! Plain Old Data type for the Pedersen commitment scheme.

use {crate::RISTRETTO_POINT_LEN, std::fmt};
#[cfg(not(target_os = "solana"))]
use {
    crate::{encryption::pedersen as decoded, errors::ElGamalError},
    curve25519_dalek::ristretto::CompressedRistretto,
};

/// Byte length of a Pedersen commitment
pub(crate) const PEDERSEN_COMMITMENT_LEN: usize = RISTRETTO_POINT_LEN;

/// The `PedersenCommitment` type as a `Pod`.
#[derive(Clone, Copy, Default, bytemuck_derive::Pod, bytemuck_derive::Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct PedersenCommitment(pub [u8; PEDERSEN_COMMITMENT_LEN]);

impl fmt::Debug for PedersenCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::PedersenCommitment> for PedersenCommitment {
    fn from(decoded_commitment: decoded::PedersenCommitment) -> Self {
        Self(decoded_commitment.to_bytes())
    }
}

// For proof verification, interpret pod::PedersenCommitment directly as CompressedRistretto
#[cfg(not(target_os = "solana"))]
impl From<PedersenCommitment> for CompressedRistretto {
    fn from(pod_commitment: PedersenCommitment) -> Self {
        Self(pod_commitment.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PedersenCommitment> for decoded::PedersenCommitment {
    type Error = ElGamalError;

    fn try_from(pod_commitment: PedersenCommitment) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_commitment.0).ok_or(ElGamalError::CiphertextDeserialization)
    }
}
