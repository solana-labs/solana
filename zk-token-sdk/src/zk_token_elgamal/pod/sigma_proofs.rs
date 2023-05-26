use crate::zk_token_elgamal::pod::{Pod, Zeroable};

/// Serialization of `CiphertextCommitmentEqualityProof`
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCommitmentEqualityProof(pub [u8; 192]);

// `CiphertextCommitmentEqualityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for CiphertextCommitmentEqualityProof {}
unsafe impl Pod for CiphertextCommitmentEqualityProof {}

/// Serialization of `CiphertextCiphertextEqualityProof`
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CiphertextCiphertextEqualityProof(pub [u8; 224]);

// `CiphertextCiphertextEqualityProof` is a Pod and Zeroable.
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
