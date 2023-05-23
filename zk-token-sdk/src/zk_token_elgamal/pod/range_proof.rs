use crate::{
    errors::ProofVerificationError,
    pod::{Pod, Zeroable},
    range_proof::{self, errors::RangeProofError},
};

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

#[cfg(not(target_os = "solana"))]
impl TryFrom<range_proof::RangeProof> for RangeProof64 {
    type Error = RangeProofError;

    fn try_from(proof: range_proof::RangeProof) -> Result<Self, Self::Error> {
        if proof.ipp_proof.serialized_size() != 448 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let mut buf = [0_u8; 672];
        buf[..32].copy_from_slice(proof.A.as_bytes());
        buf[32..64].copy_from_slice(proof.S.as_bytes());
        buf[64..96].copy_from_slice(proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(proof.e_blinding.as_bytes());
        buf[224..672].copy_from_slice(&proof.ipp_proof.to_bytes());
        Ok(RangeProof64(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProof64> for range_proof::RangeProof {
    type Error = RangeProofError;

    fn try_from(pod: RangeProof64) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<range_proof::RangeProof> for RangeProof128 {
    type Error = RangeProofError;

    fn try_from(proof: range_proof::RangeProof) -> Result<Self, Self::Error> {
        if proof.ipp_proof.serialized_size() != 512 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let mut buf = [0_u8; 736];
        buf[..32].copy_from_slice(proof.A.as_bytes());
        buf[32..64].copy_from_slice(proof.S.as_bytes());
        buf[64..96].copy_from_slice(proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(proof.e_blinding.as_bytes());
        buf[224..736].copy_from_slice(&proof.ipp_proof.to_bytes());
        Ok(RangeProof128(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProof128> for range_proof::RangeProof {
    type Error = RangeProofError;

    fn try_from(pod: RangeProof128) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<range_proof::RangeProof> for RangeProof256 {
    type Error = RangeProofError;

    fn try_from(proof: range_proof::RangeProof) -> Result<Self, Self::Error> {
        if proof.ipp_proof.serialized_size() != 576 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let mut buf = [0_u8; 800];
        buf[..32].copy_from_slice(proof.A.as_bytes());
        buf[32..64].copy_from_slice(proof.S.as_bytes());
        buf[64..96].copy_from_slice(proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(proof.e_blinding.as_bytes());
        buf[224..800].copy_from_slice(&proof.ipp_proof.to_bytes());
        Ok(RangeProof256(buf))
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<RangeProof256> for range_proof::RangeProof {
    type Error = RangeProofError;

    fn try_from(pod: RangeProof256) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}
