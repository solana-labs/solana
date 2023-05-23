use crate::{
    errors::ProofVerificationError,
    range_proof::{self, errors::RangeProofError},
    zk_token_elgamal::pod::{Pod, Zeroable},
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{encryption::pedersen::Pedersen, range_proof::RangeProof},
        merlin::Transcript,
        std::convert::TryInto,
    };

    #[test]
    fn test_pod_range_proof_64() {
        let (comm, open) = Pedersen::new(55_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::new(vec![55], vec![64], vec![&open], &mut transcript_create);

        let proof_serialized: RangeProof64 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(vec![&comm], vec![64], &mut transcript_verify)
            .is_ok());

        // should fail to serialize to RangeProof128
        let proof = RangeProof::new(vec![55], vec![64], vec![&open], &mut transcript_create);

        assert!(TryInto::<RangeProof128>::try_into(proof).is_err());
    }

    #[test]
    fn test_pod_range_proof_128() {
        let (comm_1, open_1) = Pedersen::new(55_u64);
        let (comm_2, open_2) = Pedersen::new(77_u64);
        let (comm_3, open_3) = Pedersen::new(99_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::new(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        );

        let proof_serialized: RangeProof128 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(
                vec![&comm_1, &comm_2, &comm_3],
                vec![64, 32, 32],
                &mut transcript_verify,
            )
            .is_ok());

        // should fail to serialize to RangeProof64
        let proof = RangeProof::new(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        );

        assert!(TryInto::<RangeProof64>::try_into(proof).is_err());
    }
}
