//! Plain Old Data types for sigma proofs.

#[cfg(not(target_os = "solana"))]
use crate::sigma_proofs::{
    batched_grouped_ciphertext_validity::{
        BatchedGroupedCiphertext2HandlesValidityProof,
        BatchedGroupedCiphertext3HandlesValidityProof,
    },
    ciphertext_ciphertext_equality::CiphertextCiphertextEqualityProof,
    ciphertext_commitment_equality::CiphertextCommitmentEqualityProof,
    grouped_ciphertext_validity::{
        GroupedCiphertext2HandlesValidityProof, GroupedCiphertext3HandlesValidityProof,
    },
    percentage_with_cap::PercentageWithCapProof,
    pubkey::PubkeyValidityProof,
    zero_ciphertext::ZeroCiphertextProof,
};
use {
    crate::sigma_proofs::{errors::*, *},
    bytemuck::{Pod, Zeroable},
};

/// The `CiphertextCommitmentEqualityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodCiphertextCommitmentEqualityProof(
    pub(crate) [u8; CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<CiphertextCommitmentEqualityProof> for PodCiphertextCommitmentEqualityProof {
    fn from(decoded_proof: CiphertextCommitmentEqualityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodCiphertextCommitmentEqualityProof> for CiphertextCommitmentEqualityProof {
    type Error = EqualityProofVerificationError;

    fn try_from(pod_proof: PodCiphertextCommitmentEqualityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `CiphertextCiphertextEqualityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodCiphertextCiphertextEqualityProof(
    pub(crate) [u8; CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<CiphertextCiphertextEqualityProof> for PodCiphertextCiphertextEqualityProof {
    fn from(decoded_proof: CiphertextCiphertextEqualityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodCiphertextCiphertextEqualityProof> for CiphertextCiphertextEqualityProof {
    type Error = EqualityProofVerificationError;

    fn try_from(pod_proof: PodCiphertextCiphertextEqualityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `GroupedCiphertext2HandlesValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodGroupedCiphertext2HandlesValidityProof(
    pub(crate) [u8; GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<GroupedCiphertext2HandlesValidityProof> for PodGroupedCiphertext2HandlesValidityProof {
    fn from(decoded_proof: GroupedCiphertext2HandlesValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}
#[cfg(not(target_os = "solana"))]

impl TryFrom<PodGroupedCiphertext2HandlesValidityProof> for GroupedCiphertext2HandlesValidityProof {
    type Error = ValidityProofVerificationError;

    fn try_from(pod_proof: PodGroupedCiphertext2HandlesValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `GroupedCiphertext3HandlesValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodGroupedCiphertext3HandlesValidityProof(
    pub(crate) [u8; GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<GroupedCiphertext3HandlesValidityProof> for PodGroupedCiphertext3HandlesValidityProof {
    fn from(decoded_proof: GroupedCiphertext3HandlesValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodGroupedCiphertext3HandlesValidityProof> for GroupedCiphertext3HandlesValidityProof {
    type Error = ValidityProofVerificationError;

    fn try_from(pod_proof: PodGroupedCiphertext3HandlesValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `BatchedGroupedCiphertext2HandlesValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodBatchedGroupedCiphertext2HandlesValidityProof(
    pub(crate) [u8; BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<BatchedGroupedCiphertext2HandlesValidityProof>
    for PodBatchedGroupedCiphertext2HandlesValidityProof
{
    fn from(decoded_proof: BatchedGroupedCiphertext2HandlesValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodBatchedGroupedCiphertext2HandlesValidityProof>
    for BatchedGroupedCiphertext2HandlesValidityProof
{
    type Error = ValidityProofVerificationError;

    fn try_from(
        pod_proof: PodBatchedGroupedCiphertext2HandlesValidityProof,
    ) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `BatchedGroupedCiphertext3HandlesValidityProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodBatchedGroupedCiphertext3HandlesValidityProof(
    pub(crate) [u8; BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN],
);

#[cfg(not(target_os = "solana"))]
impl From<BatchedGroupedCiphertext3HandlesValidityProof>
    for PodBatchedGroupedCiphertext3HandlesValidityProof
{
    fn from(decoded_proof: BatchedGroupedCiphertext3HandlesValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodBatchedGroupedCiphertext3HandlesValidityProof>
    for BatchedGroupedCiphertext3HandlesValidityProof
{
    type Error = ValidityProofVerificationError;

    fn try_from(
        pod_proof: PodBatchedGroupedCiphertext3HandlesValidityProof,
    ) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `ZeroCiphertextProof` type as a `Pod`.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodZeroCiphertextProof(pub(crate) [u8; ZERO_CIPHERTEXT_PROOF_LEN]);

#[cfg(not(target_os = "solana"))]
impl From<ZeroCiphertextProof> for PodZeroCiphertextProof {
    fn from(decoded_proof: ZeroCiphertextProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodZeroCiphertextProof> for ZeroCiphertextProof {
    type Error = ZeroCiphertextProofVerificationError;

    fn try_from(pod_proof: PodZeroCiphertextProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `PercentageWithCapProof` type as a `Pod`.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodPercentageWithCapProof(pub(crate) [u8; PERCENTAGE_WITH_CAP_PROOF_LEN]);

#[cfg(not(target_os = "solana"))]
impl From<PercentageWithCapProof> for PodPercentageWithCapProof {
    fn from(decoded_proof: PercentageWithCapProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodPercentageWithCapProof> for PercentageWithCapProof {
    type Error = PercentageWithCapProofVerificationError;

    fn try_from(pod_proof: PodPercentageWithCapProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `PubkeyValidityProof` type as a `Pod`.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodPubkeyValidityProof(pub(crate) [u8; PUBKEY_VALIDITY_PROOF_LEN]);

#[cfg(not(target_os = "solana"))]
impl From<PubkeyValidityProof> for PodPubkeyValidityProof {
    fn from(decoded_proof: PubkeyValidityProof) -> Self {
        Self(decoded_proof.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodPubkeyValidityProof> for PubkeyValidityProof {
    type Error = PubkeyValidityProofVerificationError;

    fn try_from(pod_proof: PodPubkeyValidityProof) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

// The sigma proof pod types are wrappers for byte arrays, which are both `Pod` and `Zeroable`. However,
// the marker traits `bytemuck::Pod` and `bytemuck::Zeroable` can only be derived for power-of-two
// length byte arrays. Directly implement these traits for the sigma proof pod types.
unsafe impl Zeroable for PodCiphertextCommitmentEqualityProof {}
unsafe impl Pod for PodCiphertextCommitmentEqualityProof {}

unsafe impl Zeroable for PodCiphertextCiphertextEqualityProof {}
unsafe impl Pod for PodCiphertextCiphertextEqualityProof {}

unsafe impl Zeroable for PodGroupedCiphertext2HandlesValidityProof {}
unsafe impl Pod for PodGroupedCiphertext2HandlesValidityProof {}

unsafe impl Zeroable for PodGroupedCiphertext3HandlesValidityProof {}
unsafe impl Pod for PodGroupedCiphertext3HandlesValidityProof {}

unsafe impl Zeroable for PodBatchedGroupedCiphertext2HandlesValidityProof {}
unsafe impl Pod for PodBatchedGroupedCiphertext2HandlesValidityProof {}

unsafe impl Zeroable for PodBatchedGroupedCiphertext3HandlesValidityProof {}
unsafe impl Pod for PodBatchedGroupedCiphertext3HandlesValidityProof {}

unsafe impl Zeroable for PodZeroCiphertextProof {}
unsafe impl Pod for PodZeroCiphertextProof {}
