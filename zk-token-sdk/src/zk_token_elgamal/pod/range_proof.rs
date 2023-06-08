//! Plain Old Data types for range proofs.

use crate::zk_token_elgamal::pod::{Pod, Zeroable};
#[cfg(not(target_os = "solana"))]
use crate::{
    errors::ProofVerificationError,
    range_proof::{self as decoded, errors::RangeProofError},
};

/// The `RangeProof` type as a `Pod` restricted to proofs on 64-bit numbers.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RangeProofU64(pub [u8; 672]);

#[cfg(not(target_os = "solana"))]
impl TryFrom<decoded::RangeProof> for RangeProofU64 {
    type Error = RangeProofError;

    fn try_from(decoded_proof: decoded::RangeProof) -> Result<Self, Self::Error> {
        if decoded_proof.ipp_proof.serialized_size() != 448 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let mut buf = [0_u8; 672];
        buf[..32].copy_from_slice(decoded_proof.A.as_bytes());
        buf[32..64].copy_from_slice(decoded_proof.S.as_bytes());
        buf[64..96].copy_from_slice(decoded_proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(decoded_proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(decoded_proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(decoded_proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(decoded_proof.e_blinding.as_bytes());
        buf[224..672].copy_from_slice(&decoded_proof.ipp_proof.to_bytes());
        Ok(RangeProofU64(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProofU64> for decoded::RangeProof {
    type Error = RangeProofError;

    fn try_from(pod_proof: RangeProofU64) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `RangeProof` type as a `Pod` restricted to proofs on 128-bit numbers.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RangeProofU128(pub [u8; 736]);

#[cfg(not(target_os = "solana"))]
impl TryFrom<decoded::RangeProof> for RangeProofU128 {
    type Error = RangeProofError;

    fn try_from(decoded_proof: decoded::RangeProof) -> Result<Self, Self::Error> {
        if decoded_proof.ipp_proof.serialized_size() != 512 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let mut buf = [0_u8; 736];
        buf[..32].copy_from_slice(decoded_proof.A.as_bytes());
        buf[32..64].copy_from_slice(decoded_proof.S.as_bytes());
        buf[64..96].copy_from_slice(decoded_proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(decoded_proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(decoded_proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(decoded_proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(decoded_proof.e_blinding.as_bytes());
        buf[224..736].copy_from_slice(&decoded_proof.ipp_proof.to_bytes());
        Ok(RangeProofU128(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProofU128> for decoded::RangeProof {
    type Error = RangeProofError;

    fn try_from(pod_proof: RangeProofU128) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

/// The `RangeProof` type as a `Pod` restricted to proofs on 256-bit numbers.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RangeProofU256(pub [u8; 800]);

#[cfg(not(target_os = "solana"))]
impl TryFrom<decoded::RangeProof> for RangeProofU256 {
    type Error = RangeProofError;

    fn try_from(decoded_proof: decoded::RangeProof) -> Result<Self, Self::Error> {
        if decoded_proof.ipp_proof.serialized_size() != 576 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let mut buf = [0_u8; 800];
        buf[..32].copy_from_slice(decoded_proof.A.as_bytes());
        buf[32..64].copy_from_slice(decoded_proof.S.as_bytes());
        buf[64..96].copy_from_slice(decoded_proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(decoded_proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(decoded_proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(decoded_proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(decoded_proof.e_blinding.as_bytes());
        buf[224..800].copy_from_slice(&decoded_proof.ipp_proof.to_bytes());
        Ok(RangeProofU256(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProofU256> for decoded::RangeProof {
    type Error = RangeProofError;

    fn try_from(pod_proof: RangeProofU256) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

// The range proof pod types are wrappers for byte arrays, which are both `Pod` and `Zeroable`. However,
// the marker traits `bytemuck::Pod` and `bytemuck::Zeroable` can only be derived for power-of-two
// length byte arrays. Directly implement these traits for the range proof pod types.
unsafe impl Zeroable for RangeProofU64 {}
unsafe impl Pod for RangeProofU64 {}

unsafe impl Zeroable for RangeProofU128 {}
unsafe impl Pod for RangeProofU128 {}

unsafe impl Zeroable for RangeProofU256 {}
unsafe impl Pod for RangeProofU256 {}
