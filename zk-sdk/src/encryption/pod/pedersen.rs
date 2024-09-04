//! Plain Old Data type for the Pedersen commitment scheme.

#[cfg(not(target_os = "solana"))]
use {
    crate::{encryption::pedersen::PedersenCommitment, errors::ElGamalError},
    curve25519_dalek::ristretto::CompressedRistretto,
};
use {
    crate::{
        encryption::PEDERSEN_COMMITMENT_LEN,
        pod::{impl_from_bytes, impl_from_str},
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    bytemuck_derive::{Pod, Zeroable},
    std::fmt,
};

/// Maximum length of a base64 encoded ElGamal public key
const PEDERSEN_COMMITMENT_MAX_BASE64_LEN: usize = 44;

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

impl fmt::Display for PodPedersenCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodPedersenCommitment,
    BYTES_LEN = PEDERSEN_COMMITMENT_LEN,
    BASE64_LEN = PEDERSEN_COMMITMENT_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodPedersenCommitment,
    BYTES_LEN = PEDERSEN_COMMITMENT_LEN
);

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
