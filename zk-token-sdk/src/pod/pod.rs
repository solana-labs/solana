pub use bytemuck::{Pod, Zeroable};
use {
    crate::pod::{elgamal::*, pedersen::*, PodU16, PodU64},
    base64::{prelude::BASE64_STANDARD, Engine},
    std::fmt,
};

#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct CompressedRistretto(pub [u8; 32]);

/// Serialization of `CiphertextCommitmentEqualityProof`
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCommitmentEqualityProof(pub [u8; 192]);

// `CiphertextCommitmentEqualityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for CiphertextCommitmentEqualityProof {}
unsafe impl Pod for CiphertextCommitmentEqualityProof {}

/// Serialization of `CtxtCtxtEqualityProof`
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCiphertextEqualityProof(pub [u8; 224]);

// `CtxtCtxtEqualityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for CiphertextCiphertextEqualityProof {}
unsafe impl Pod for CiphertextCiphertextEqualityProof {}

/// Serialization of validity proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ValidityProof(pub [u8; 160]);

// `ValidityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for ValidityProof {}
unsafe impl Pod for ValidityProof {}

/// Serialization of aggregated validity proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct AggregatedValidityProof(pub [u8; 160]);

// `AggregatedValidityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for AggregatedValidityProof {}
unsafe impl Pod for AggregatedValidityProof {}

/// Serialization of zero balance proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ZeroBalanceProof(pub [u8; 96]);

// `ZeroBalanceProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for ZeroBalanceProof {}
unsafe impl Pod for ZeroBalanceProof {}

/// Serialization of fee sigma proof
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct FeeSigmaProof(pub [u8; 256]);

/// Serialization of public-key sigma proof
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct PubkeyValidityProof(pub [u8; 64]);

/// Serialization of range proofs for 64-bit numbers (for `Withdraw` instruction)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RangeProof64(pub [u8; 672]);

// `PodRangeProof64` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for RangeProof64 {}
unsafe impl Pod for RangeProof64 {}

/// Serialization of range proofs for 128-bit numbers (for `TransferRangeProof` instruction)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RangeProof128(pub [u8; 736]);

// `PodRangeProof128` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for RangeProof128 {}
unsafe impl Pod for RangeProof128 {}

/// Serialization of range proofs for 128-bit numbers (for `TransferRangeProof` instruction)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RangeProof256(pub [u8; 800]);

// `PodRangeProof256` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for RangeProof256 {}
unsafe impl Pod for RangeProof256 {}

// TODO: refactor this code into the instruction module
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferPubkeys {
    pub source_pubkey: ElGamalPubkey,
    pub destination_pubkey: ElGamalPubkey,
    pub auditor_pubkey: ElGamalPubkey,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferWithFeePubkeys {
    pub source_pubkey: ElGamalPubkey,
    pub destination_pubkey: ElGamalPubkey,
    pub auditor_pubkey: ElGamalPubkey,
    pub withdraw_withheld_authority_pubkey: ElGamalPubkey,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferAmountEncryption {
    pub commitment: PedersenCommitment,
    pub source_handle: DecryptHandle,
    pub destination_handle: DecryptHandle,
    pub auditor_handle: DecryptHandle,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeEncryption {
    pub commitment: PedersenCommitment,
    pub destination_handle: DecryptHandle,
    pub withdraw_withheld_authority_handle: DecryptHandle,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: PodU16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: PodU64,
}
