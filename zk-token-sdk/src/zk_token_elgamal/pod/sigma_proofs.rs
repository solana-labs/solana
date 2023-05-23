use crate::{
    sigma_proofs::{
        ctxt_comm_equality_proof, ctxt_ctxt_equality_proof, errors::*, fee_proof, pubkey_proof,
        validity_proof, zero_balance_proof,
    },
    zk_token_elgamal::pod::{Pod, Zeroable},
};

/// Serialization of `CiphertextCommitmentEqualityProof`
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCommitmentEqualityProof(pub [u8; 192]);

// `CiphertextCommitmentEqualityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for CiphertextCommitmentEqualityProof {}
unsafe impl Pod for CiphertextCommitmentEqualityProof {}

#[cfg(not(target_os = "solana"))]
impl From<ctxt_comm_equality_proof::CiphertextCommitmentEqualityProof>
    for CiphertextCommitmentEqualityProof
{
    fn from(proof: ctxt_comm_equality_proof::CiphertextCommitmentEqualityProof) -> Self {
        Self(proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<CiphertextCommitmentEqualityProof>
    for ctxt_comm_equality_proof::CiphertextCommitmentEqualityProof
{
    type Error = EqualityProofError;

    fn try_from(pod: CiphertextCommitmentEqualityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

/// Serialization of `CtxtCtxtEqualityProof`
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCiphertextEqualityProof(pub [u8; 224]);

// `CtxtCtxtEqualityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for CiphertextCiphertextEqualityProof {}
unsafe impl Pod for CiphertextCiphertextEqualityProof {}

#[cfg(not(target_os = "solana"))]
impl From<ctxt_ctxt_equality_proof::CiphertextCiphertextEqualityProof>
    for CiphertextCiphertextEqualityProof
{
    fn from(proof: ctxt_ctxt_equality_proof::CiphertextCiphertextEqualityProof) -> Self {
        Self(proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<CiphertextCiphertextEqualityProof>
    for ctxt_ctxt_equality_proof::CiphertextCiphertextEqualityProof
{
    type Error = EqualityProofError;

    fn try_from(pod: CiphertextCiphertextEqualityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

/// Serialization of validity proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ValidityProof(pub [u8; 160]);

// `ValidityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for ValidityProof {}
unsafe impl Pod for ValidityProof {}

#[cfg(not(target_os = "solana"))]
impl From<validity_proof::ValidityProof> for ValidityProof {
    fn from(proof: validity_proof::ValidityProof) -> Self {
        Self(proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ValidityProof> for validity_proof::ValidityProof {
    type Error = ValidityProofError;

    fn try_from(pod: ValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

/// Serialization of aggregated validity proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct AggregatedValidityProof(pub [u8; 160]);

// `AggregatedValidityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for AggregatedValidityProof {}
unsafe impl Pod for AggregatedValidityProof {}

#[cfg(not(target_os = "solana"))]
impl From<validity_proof::AggregatedValidityProof> for AggregatedValidityProof {
    fn from(proof: validity_proof::AggregatedValidityProof) -> Self {
        Self(proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<AggregatedValidityProof> for validity_proof::AggregatedValidityProof {
    type Error = ValidityProofError;

    fn try_from(pod: AggregatedValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

/// Serialization of zero balance proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ZeroBalanceProof(pub [u8; 96]);

// `ZeroBalanceProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for ZeroBalanceProof {}
unsafe impl Pod for ZeroBalanceProof {}

#[cfg(not(target_os = "solana"))]
impl From<zero_balance_proof::ZeroBalanceProof> for ZeroBalanceProof {
    fn from(proof: zero_balance_proof::ZeroBalanceProof) -> Self {
        Self(proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ZeroBalanceProof> for zero_balance_proof::ZeroBalanceProof {
    type Error = ZeroBalanceProofError;

    fn try_from(pod: ZeroBalanceProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

/// Serialization of fee sigma proof
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct FeeSigmaProof(pub [u8; 256]);

#[cfg(not(target_os = "solana"))]
impl From<fee_proof::FeeSigmaProof> for FeeSigmaProof {
    fn from(proof: fee_proof::FeeSigmaProof) -> Self {
        Self(proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<FeeSigmaProof> for fee_proof::FeeSigmaProof {
    type Error = FeeSigmaProofError;

    fn try_from(pod: FeeSigmaProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

/// Serialization of public-key sigma proof
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct PubkeyValidityProof(pub [u8; 64]);

#[cfg(not(target_os = "solana"))]
impl From<pubkey_proof::PubkeyValidityProof> for PubkeyValidityProof {
    fn from(proof: pubkey_proof::PubkeyValidityProof) -> Self {
        Self(proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PubkeyValidityProof> for pubkey_proof::PubkeyValidityProof {
    type Error = PubkeyValidityProofError;

    fn try_from(pod: PubkeyValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}
