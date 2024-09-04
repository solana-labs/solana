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
    pubkey_validity::PubkeyValidityProof,
    zero_ciphertext::ZeroCiphertextProof,
};
use {
    crate::{
        pod::{impl_from_bytes, impl_from_str},
        sigma_proofs::{errors::*, *},
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    bytemuck::{Pod, Zeroable},
    std::fmt,
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

const CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_MAX_BASE64_LEN: usize = 256;

impl fmt::Display for PodCiphertextCommitmentEqualityProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodCiphertextCommitmentEqualityProof,
    BYTES_LEN = CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_LEN,
    BASE64_LEN = CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodCiphertextCommitmentEqualityProof,
    BYTES_LEN = CIPHERTEXT_COMMITMENT_EQUALITY_PROOF_LEN
);

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

const CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_MAX_BASE64_LEN: usize = 300;

impl fmt::Display for PodCiphertextCiphertextEqualityProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodCiphertextCiphertextEqualityProof,
    BYTES_LEN = CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN,
    BASE64_LEN = CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodCiphertextCiphertextEqualityProof,
    BYTES_LEN = CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN
);

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

const GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN: usize = 216;

impl fmt::Display for PodGroupedCiphertext2HandlesValidityProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodGroupedCiphertext2HandlesValidityProof,
    BYTES_LEN = GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN,
    BASE64_LEN = GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodGroupedCiphertext2HandlesValidityProof,
    BYTES_LEN = GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN
);

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

const GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN: usize = 256;

impl fmt::Display for PodGroupedCiphertext3HandlesValidityProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodGroupedCiphertext3HandlesValidityProof,
    BYTES_LEN = GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN,
    BASE64_LEN = GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodGroupedCiphertext3HandlesValidityProof,
    BYTES_LEN = GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN
);

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

const BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN: usize = 216;

impl fmt::Display for PodBatchedGroupedCiphertext2HandlesValidityProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodBatchedGroupedCiphertext2HandlesValidityProof,
    BYTES_LEN = BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN,
    BASE64_LEN = BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodBatchedGroupedCiphertext2HandlesValidityProof,
    BYTES_LEN = BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_PROOF_LEN
);

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

const BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN: usize = 256;

impl fmt::Display for PodBatchedGroupedCiphertext3HandlesValidityProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodBatchedGroupedCiphertext3HandlesValidityProof,
    BYTES_LEN = BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN,
    BASE64_LEN = BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodBatchedGroupedCiphertext3HandlesValidityProof,
    BYTES_LEN = BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN
);

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

const ZERO_CIPHERTEXT_PROOF_MAX_BASE64_LEN: usize = 128;

impl fmt::Display for PodZeroCiphertextProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodZeroCiphertextProof,
    BYTES_LEN = ZERO_CIPHERTEXT_PROOF_LEN,
    BASE64_LEN = ZERO_CIPHERTEXT_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodZeroCiphertextProof,
    BYTES_LEN = ZERO_CIPHERTEXT_PROOF_LEN
);

/// The `PercentageWithCapProof` type as a `Pod`.
#[derive(Clone, Copy, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
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

const PERCENTAGE_WITH_CAP_PROOF_MAX_BASE64_LEN: usize = 344;

impl fmt::Display for PodPercentageWithCapProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodPercentageWithCapProof,
    BYTES_LEN = PERCENTAGE_WITH_CAP_PROOF_LEN,
    BASE64_LEN = PERCENTAGE_WITH_CAP_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodPercentageWithCapProof,
    BYTES_LEN = PERCENTAGE_WITH_CAP_PROOF_LEN
);

/// The `PubkeyValidityProof` type as a `Pod`.
#[derive(Clone, Copy, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
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

const PUBKEY_VALIDITY_PROOF_MAX_BASE64_LEN: usize = 88;

impl fmt::Display for PodPubkeyValidityProof {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodPubkeyValidityProof,
    BYTES_LEN = PUBKEY_VALIDITY_PROOF_LEN,
    BASE64_LEN = PUBKEY_VALIDITY_PROOF_MAX_BASE64_LEN
);

impl_from_bytes!(
    TYPE = PodPubkeyValidityProof,
    BYTES_LEN = PUBKEY_VALIDITY_PROOF_LEN
);

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
