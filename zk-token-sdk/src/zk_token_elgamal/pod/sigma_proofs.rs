//! Plain Old Data types for sigma proofs.

#[cfg(not(target_os = "solana"))]
use crate::sigma_proofs::{
    ciphertext_ciphertext_equality_proof::CiphertextCiphertextEqualityProof as DecodedCiphertextCiphertextEqualityProof,
    ciphertext_commitment_equality_proof::CiphertextCommitmentEqualityProof as DecodedCiphertextCommitmentEqualityProof,
    errors::*,
    fee_proof::FeeSigmaProof as DecodedFeeSigmaProof,
    pubkey_proof::PubkeyValidityProof as DecodedPubkeyValidityProof,
    validity_proof::{
        AggregatedValidityProof as DecodedAggregatedValidityProof,
        ValidityProof as DecodedValidityProof,
    },
    zero_balance_proof::ZeroBalanceProof as DecodedZeroBalanceProof,
};
use crate::zk_token_elgamal::pod::{Pod, Zeroable};

/// The `CiphertextCommitmentEqualityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCommitmentEqualityProof(pub [u8; 192]);

#[cfg(not(target_os = "solana"))]
impl From<DecodedCiphertextCommitmentEqualityProof> for CiphertextCommitmentEqualityProof {
    fn from(decoded_proof: DecodedCiphertextCommitmentEqualityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<CiphertextCommitmentEqualityProof> for DecodedCiphertextCommitmentEqualityProof {
    type Error = EqualityProofError;

    fn try_from(pod_proof: CiphertextCommitmentEqualityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `CiphertextCiphertextEqualityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCiphertextEqualityProof(pub [u8; 224]);

#[cfg(not(target_os = "solana"))]
impl From<DecodedCiphertextCiphertextEqualityProof> for CiphertextCiphertextEqualityProof {
    fn from(decoded_proof: DecodedCiphertextCiphertextEqualityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<CiphertextCiphertextEqualityProof> for DecodedCiphertextCiphertextEqualityProof {
    type Error = EqualityProofError;

    fn try_from(pod_proof: CiphertextCiphertextEqualityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `ValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ValidityProof(pub [u8; 160]);

#[cfg(not(target_os = "solana"))]
impl From<DecodedValidityProof> for ValidityProof {
    fn from(decoded_proof: DecodedValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ValidityProof> for DecodedValidityProof {
    type Error = ValidityProofError;

    fn try_from(pod_proof: ValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `AggregatedValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct AggregatedValidityProof(pub [u8; 160]);

#[cfg(not(target_os = "solana"))]
impl From<DecodedAggregatedValidityProof> for AggregatedValidityProof {
    fn from(decoded_proof: DecodedAggregatedValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<AggregatedValidityProof> for DecodedAggregatedValidityProof {
    type Error = ValidityProofError;

    fn try_from(pod_proof: AggregatedValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `ZeroBalanceProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ZeroBalanceProof(pub [u8; 96]);

#[cfg(not(target_os = "solana"))]
impl From<DecodedZeroBalanceProof> for ZeroBalanceProof {
    fn from(decoded_proof: DecodedZeroBalanceProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ZeroBalanceProof> for DecodedZeroBalanceProof {
    type Error = ZeroBalanceProofError;

    fn try_from(pod_proof: ZeroBalanceProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `FeeSigmaProof` type as a `Pod`.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct FeeSigmaProof(pub [u8; 256]);

#[cfg(not(target_os = "solana"))]
impl From<DecodedFeeSigmaProof> for FeeSigmaProof {
    fn from(decoded_proof: DecodedFeeSigmaProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<FeeSigmaProof> for DecodedFeeSigmaProof {
    type Error = FeeSigmaProofError;

    fn try_from(pod_proof: FeeSigmaProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `PubkeyValidityProof` type as a `Pod`.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct PubkeyValidityProof(pub [u8; 64]);

#[cfg(not(target_os = "solana"))]
impl From<DecodedPubkeyValidityProof> for PubkeyValidityProof {
    fn from(decoded_proof: DecodedPubkeyValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PubkeyValidityProof> for DecodedPubkeyValidityProof {
    type Error = PubkeyValidityProofError;

    fn try_from(pod_proof: PubkeyValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

// The sigma proof pod types are wrappers for byte arrays, which are both `Pod` and `Zeroable`. However,
// the marker traits `bytemuck::Pod` and `bytemuck::Zeroable` can only be derived for power-of-two
// length byte arrays. Directly implement these traits for the sigma proof pod types.
unsafe impl Zeroable for CiphertextCommitmentEqualityProof {}
unsafe impl Pod for CiphertextCommitmentEqualityProof {}

unsafe impl Zeroable for CiphertextCiphertextEqualityProof {}
unsafe impl Pod for CiphertextCiphertextEqualityProof {}

unsafe impl Zeroable for ValidityProof {}
unsafe impl Pod for ValidityProof {}

unsafe impl Zeroable for AggregatedValidityProof {}
unsafe impl Pod for AggregatedValidityProof {}

unsafe impl Zeroable for ZeroBalanceProof {}
unsafe impl Pod for ZeroBalanceProof {}
