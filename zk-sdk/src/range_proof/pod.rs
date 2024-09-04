//! Plain Old Data types for range proofs.

#[cfg(not(target_os = "solana"))]
use crate::{
    range_proof::{errors::RangeProofVerificationError, RangeProof},
    UNIT_LEN,
};
use {
    crate::{
        pod::{impl_from_bytes, impl_from_str},
        range_proof::*,
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    bytemuck::{Pod, Zeroable},
    std::fmt,
};

/// The `RangeProof` type as a `Pod` restricted to proofs on 64-bit numbers.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodRangeProofU64(pub(crate) [u8; RANGE_PROOF_U64_LEN]);

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProof> for PodRangeProofU64 {
    type Error = RangeProofVerificationError;

    fn try_from(decoded_proof: RangeProof) -> Result<Self, Self::Error> {
        if decoded_proof.ipp_proof.serialized_size() != INNER_PRODUCT_PROOF_U64_LEN {
            return Err(RangeProofVerificationError::Deserialization);
        }

        let mut buf = [0_u8; RANGE_PROOF_U64_LEN];
        copy_range_proof_modulo_inner_product_proof(&decoded_proof, &mut buf);
        buf[RANGE_PROOF_MODULO_INNER_PRODUCT_PROOF_LEN..RANGE_PROOF_U64_LEN]
            .copy_from_slice(&decoded_proof.ipp_proof.to_bytes());
        Ok(PodRangeProofU64(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodRangeProofU64> for RangeProof {
    type Error = RangeProofVerificationError;

    fn try_from(pod_proof: PodRangeProofU64) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

const RANGE_PROOF_U64_MAX_BASE64_LEN: usize = 896;

impl fmt::Display for PodRangeProofU64 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodRangeProofU64,
    BYTES_LEN = RANGE_PROOF_U64_LEN,
    BASE64_LEN = RANGE_PROOF_U64_MAX_BASE64_LEN
);

impl_from_bytes!(TYPE = PodRangeProofU64, BYTES_LEN = RANGE_PROOF_U64_LEN);

/// The `RangeProof` type as a `Pod` restricted to proofs on 128-bit numbers.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodRangeProofU128(pub(crate) [u8; RANGE_PROOF_U128_LEN]);

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProof> for PodRangeProofU128 {
    type Error = RangeProofVerificationError;

    fn try_from(decoded_proof: RangeProof) -> Result<Self, Self::Error> {
        if decoded_proof.ipp_proof.serialized_size() != INNER_PRODUCT_PROOF_U128_LEN {
            return Err(RangeProofVerificationError::Deserialization);
        }

        let mut buf = [0_u8; RANGE_PROOF_U128_LEN];
        copy_range_proof_modulo_inner_product_proof(&decoded_proof, &mut buf);
        buf[RANGE_PROOF_MODULO_INNER_PRODUCT_PROOF_LEN..RANGE_PROOF_U128_LEN]
            .copy_from_slice(&decoded_proof.ipp_proof.to_bytes());
        Ok(PodRangeProofU128(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodRangeProofU128> for RangeProof {
    type Error = RangeProofVerificationError;

    fn try_from(pod_proof: PodRangeProofU128) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

const RANGE_PROOF_U128_MAX_BASE64_LEN: usize = 984;

impl fmt::Display for PodRangeProofU128 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodRangeProofU128,
    BYTES_LEN = RANGE_PROOF_U128_LEN,
    BASE64_LEN = RANGE_PROOF_U128_MAX_BASE64_LEN
);

impl_from_bytes!(TYPE = PodRangeProofU128, BYTES_LEN = RANGE_PROOF_U128_LEN);

/// The `RangeProof` type as a `Pod` restricted to proofs on 256-bit numbers.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodRangeProofU256(pub(crate) [u8; RANGE_PROOF_U256_LEN]);

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProof> for PodRangeProofU256 {
    type Error = RangeProofVerificationError;

    fn try_from(decoded_proof: RangeProof) -> Result<Self, Self::Error> {
        if decoded_proof.ipp_proof.serialized_size() != INNER_PRODUCT_PROOF_U256_LEN {
            return Err(RangeProofVerificationError::Deserialization);
        }

        let mut buf = [0_u8; RANGE_PROOF_U256_LEN];
        copy_range_proof_modulo_inner_product_proof(&decoded_proof, &mut buf);
        buf[RANGE_PROOF_MODULO_INNER_PRODUCT_PROOF_LEN..RANGE_PROOF_U256_LEN]
            .copy_from_slice(&decoded_proof.ipp_proof.to_bytes());
        Ok(PodRangeProofU256(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<PodRangeProofU256> for RangeProof {
    type Error = RangeProofVerificationError;

    fn try_from(pod_proof: PodRangeProofU256) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_proof.0)
    }
}

const RANGE_PROOF_U256_MAX_BASE64_LEN: usize = 1068;

impl fmt::Display for PodRangeProofU256 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = PodRangeProofU256,
    BYTES_LEN = RANGE_PROOF_U256_LEN,
    BASE64_LEN = RANGE_PROOF_U256_MAX_BASE64_LEN
);

impl_from_bytes!(TYPE = PodRangeProofU256, BYTES_LEN = RANGE_PROOF_U256_LEN);

#[cfg(not(target_os = "solana"))]
fn copy_range_proof_modulo_inner_product_proof(proof: &RangeProof, buf: &mut [u8]) {
    let mut chunks = buf.chunks_mut(UNIT_LEN);
    chunks.next().unwrap().copy_from_slice(proof.A.as_bytes());
    chunks.next().unwrap().copy_from_slice(proof.S.as_bytes());
    chunks.next().unwrap().copy_from_slice(proof.T_1.as_bytes());
    chunks.next().unwrap().copy_from_slice(proof.T_2.as_bytes());
    chunks.next().unwrap().copy_from_slice(proof.t_x.as_bytes());
    chunks
        .next()
        .unwrap()
        .copy_from_slice(proof.t_x_blinding.as_bytes());
    chunks
        .next()
        .unwrap()
        .copy_from_slice(proof.e_blinding.as_bytes());
}

// The range proof pod types are wrappers for byte arrays, which are both `Pod` and `Zeroable`. However,
// the marker traits `bytemuck::Pod` and `bytemuck::Zeroable` can only be derived for power-of-two
// length byte arrays. Directly implement these traits for the range proof pod types.
unsafe impl Zeroable for PodRangeProofU64 {}
unsafe impl Pod for PodRangeProofU64 {}

unsafe impl Zeroable for PodRangeProofU128 {}
unsafe impl Pod for PodRangeProofU128 {}

unsafe impl Zeroable for PodRangeProofU256 {}
unsafe impl Pod for PodRangeProofU256 {}
