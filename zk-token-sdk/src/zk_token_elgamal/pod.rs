pub use bytemuck::{Pod, Zeroable};
use std::fmt;

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct Scalar(pub [u8; 32]);

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct CompressedRistretto(pub [u8; 32]);

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct ElGamalCiphertext(pub [u8; 64]);

impl fmt::Debug for ElGamalCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct ElGamalPubkey(pub [u8; 32]);

impl fmt::Debug for ElGamalPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct PedersenCommitment(pub [u8; 32]);

impl fmt::Debug for PedersenCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct PedersenDecryptHandle(pub [u8; 32]);

impl fmt::Debug for PedersenDecryptHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Serialization of equality proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct EqualityProof(pub [u8; 192]);

// `EqualityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for EqualityProof {}
unsafe impl Pod for EqualityProof {}

/// Serialization of validity proofs
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ValidityProof(pub [u8; 160]);

// `ValidityProof` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for ValidityProof {}
unsafe impl Pod for ValidityProof {}

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

/// Serialization for AeCiphertext
#[derive(Clone, Copy, PartialEq)]
#[repr(transparent)]
pub struct AeCiphertext(pub [u8; 36]);

// `AeCiphertext` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for AeCiphertext {}
unsafe impl Pod for AeCiphertext {}

impl fmt::Debug for AeCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
