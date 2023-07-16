//! Plain Old Data types for sigma proofs.

#[cfg(not(target_os = "solana"))]
use crate::sigma_proofs::{
    batched_grouped_ciphertext_validity_proof::BatchedGroupedCiphertext2HandlesValidityProof as DecodedBatchedGroupedCiphertext2HandlesValidityProof,
    ciphertext_ciphertext_equality_proof::CiphertextCiphertextEqualityProof as DecodedCiphertextCiphertextEqualityProof,
    ciphertext_commitment_equality_proof::CiphertextCommitmentEqualityProof as DecodedCiphertextCommitmentEqualityProof,
    errors::*, fee_proof::FeeSigmaProof as DecodedFeeSigmaProof,
    grouped_ciphertext_validity_proof::GroupedCiphertext2HandlesValidityProof as DecodedGroupedCiphertext2HandlesValidityProof,
    pubkey_proof::PubkeyValidityProof as DecodedPubkeyValidityProof,
    zero_balance_proof::ZeroBalanceProof as DecodedZeroBalanceProof,
};
use crate::zk_token_elgamal::pod::{Pod, Zeroable};

/// Byte length of a ciphertext-commitment equality proof
const CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_LEN: usize = 192;

/// Byte length of a ciphertext-ciphertext equality proof
const CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN: usize = 224;

/// Byte length of a grouped ciphertext for 2 handles validity proof
const GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN: usize = 160;

/// Byte length of a batched grouped ciphertext for 2 handles validity proof
const BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN: usize = 160;

/// Byte length of a zero-balance proof
const ZERO_BALANCE_PROOF_LEN: usize = 96;

/// Byte length of a fee sigma proof
const FEE_SIGMA_PROOF_LEN: usize = 256;

/// Byte length of a public key validity proof
const PUBKEY_VALIDITY_PROOF_LEN: usize = 64;

/// The `CiphertextCommitmentEqualityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCommitmentEqualityProof(pub [u8; CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_LEN]);

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
pub struct CiphertextCiphertextEqualityProof(pub [u8; CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN]);

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

/// The `GroupedCiphertext2HandlesValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct GroupedCiphertext2HandlesValidityProof(
    pub [u8; GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<DecodedGroupedCiphertext2HandlesValidityProof>
    for GroupedCiphertext2HandlesValidityProof
{
    fn from(decoded_proof: DecodedGroupedCiphertext2HandlesValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<GroupedCiphertext2HandlesValidityProof>
    for DecodedGroupedCiphertext2HandlesValidityProof
{
    type Error = ValidityProofError;

    fn try_from(pod_proof: GroupedCiphertext2HandlesValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `BatchedGroupedCiphertext2HandlesValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct BatchedGroupedCiphertext2HandlesValidityProof(
    pub [u8; BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<DecodedBatchedGroupedCiphertext2HandlesValidityProof>
    for BatchedGroupedCiphertext2HandlesValidityProof
{
    fn from(decoded_proof: DecodedBatchedGroupedCiphertext2HandlesValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<BatchedGroupedCiphertext2HandlesValidityProof>
    for DecodedBatchedGroupedCiphertext2HandlesValidityProof
{
    type Error = ValidityProofError;

    fn try_from(
        pod_proof: BatchedGroupedCiphertext2HandlesValidityProof,
    ) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `ZeroBalanceProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ZeroBalanceProof(pub [u8; ZERO_BALANCE_PROOF_LEN]);

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
pub struct FeeSigmaProof(pub [u8; FEE_SIGMA_PROOF_LEN]);

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
pub struct PubkeyValidityProof(pub [u8; PUBKEY_VALIDITY_PROOF_LEN]);

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

unsafe impl Zeroable for GroupedCiphertext2HandlesValidityProof {}
unsafe impl Pod for GroupedCiphertext2HandlesValidityProof {}

unsafe impl Zeroable for BatchedGroupedCiphertext2HandlesValidityProof {}
unsafe impl Pod for BatchedGroupedCiphertext2HandlesValidityProof {}

unsafe impl Zeroable for ZeroBalanceProof {}
unsafe impl Pod for ZeroBalanceProof {}
