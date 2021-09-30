use super::pod;
pub use target_arch::*;

impl From<(pod::PedersenComm, pod::PedersenDecHandle)> for pod::ElGamalCT {
    fn from((comm, decrypt_handle): (pod::PedersenComm, pod::PedersenDecHandle)) -> Self {
        let mut buf = [0_u8; 64];
        buf[..32].copy_from_slice(&comm.0);
        buf[32..].copy_from_slice(&decrypt_handle.0);
        pod::ElGamalCT(buf)
    }
}

#[cfg(not(target_arch = "bpf"))]
mod target_arch {
    use {
        super::pod,
        crate::{
            encryption::elgamal::{ElGamalCT, ElGamalPK},
            encryption::pedersen::{PedersenComm, PedersenDecHandle},
            errors::ProofError,
            range_proof::RangeProof,
        },
        curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar},
        std::{convert::TryFrom, fmt},
    };

    impl From<Scalar> for pod::Scalar {
        fn from(scalar: Scalar) -> Self {
            Self(scalar.to_bytes())
        }
    }

    impl From<pod::Scalar> for Scalar {
        fn from(pod: pod::Scalar) -> Self {
            Scalar::from_bits(pod.0)
        }
    }

    impl From<ElGamalCT> for pod::ElGamalCT {
        fn from(ct: ElGamalCT) -> Self {
            Self(ct.to_bytes())
        }
    }

    impl TryFrom<pod::ElGamalCT> for ElGamalCT {
        type Error = ProofError;

        fn try_from(ct: pod::ElGamalCT) -> Result<Self, Self::Error> {
            Self::from_bytes(&ct.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    impl fmt::Debug for pod::ElGamalCT {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }

    impl From<ElGamalPK> for pod::ElGamalPK {
        fn from(pk: ElGamalPK) -> Self {
            Self(pk.to_bytes())
        }
    }

    impl TryFrom<pod::ElGamalPK> for ElGamalPK {
        type Error = ProofError;

        fn try_from(pk: pod::ElGamalPK) -> Result<Self, Self::Error> {
            Self::from_bytes(&pk.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    impl fmt::Debug for pod::ElGamalPK {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }

    impl From<CompressedRistretto> for pod::CompressedRistretto {
        fn from(cr: CompressedRistretto) -> Self {
            Self(cr.to_bytes())
        }
    }

    impl From<pod::CompressedRistretto> for CompressedRistretto {
        fn from(pod: pod::CompressedRistretto) -> Self {
            Self(pod.0)
        }
    }

    impl From<PedersenComm> for pod::PedersenComm {
        fn from(comm: PedersenComm) -> Self {
            Self(comm.to_bytes())
        }
    }

    // For proof verification, interpret pod::PedersenComm directly as CompressedRistretto
    #[cfg(not(target_arch = "bpf"))]
    impl From<pod::PedersenComm> for CompressedRistretto {
        fn from(pod: pod::PedersenComm) -> Self {
            Self(pod.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl TryFrom<pod::PedersenComm> for PedersenComm {
        type Error = ProofError;

        fn try_from(pod: pod::PedersenComm) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl fmt::Debug for pod::PedersenComm {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl From<PedersenDecHandle> for pod::PedersenDecHandle {
        fn from(handle: PedersenDecHandle) -> Self {
            Self(handle.to_bytes())
        }
    }

    // For proof verification, interpret pod::PedersenDecHandle as CompressedRistretto
    #[cfg(not(target_arch = "bpf"))]
    impl From<pod::PedersenDecHandle> for CompressedRistretto {
        fn from(pod: pod::PedersenDecHandle) -> Self {
            Self(pod.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl TryFrom<pod::PedersenDecHandle> for PedersenDecHandle {
        type Error = ProofError;

        fn try_from(pod: pod::PedersenDecHandle) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl fmt::Debug for pod::PedersenDecHandle {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }

    impl TryFrom<RangeProof> for pod::RangeProof64 {
        type Error = ProofError;

        fn try_from(proof: RangeProof) -> Result<Self, Self::Error> {
            if proof.ipp_proof.serialized_size() != 448 {
                return Err(ProofError::VerificationError);
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
            Ok(pod::RangeProof64(buf))
        }
    }

    impl TryFrom<pod::RangeProof64> for RangeProof {
        type Error = ProofError;

        fn try_from(pod: pod::RangeProof64) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl TryFrom<RangeProof> for pod::RangeProof128 {
        type Error = ProofError;

        fn try_from(proof: RangeProof) -> Result<Self, Self::Error> {
            if proof.ipp_proof.serialized_size() != 512 {
                return Err(ProofError::VerificationError);
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
            Ok(pod::RangeProof128(buf))
        }
    }

    impl TryFrom<pod::RangeProof128> for RangeProof {
        type Error = ProofError;

        fn try_from(pod: pod::RangeProof128) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }
}

#[cfg(target_arch = "bpf")]
#[allow(unused_variables)]
mod target_arch {}

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
        let (comm, open) = Pedersen::commit(55_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::create(vec![55], vec![64], vec![&open], &mut transcript_create);

        let proof_serialized: pod::RangeProof64 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(
                vec![&comm.get_point().compress()],
                vec![64],
                &mut transcript_verify
            )
            .is_ok());

        // should fail to serialize to pod::RangeProof128
        let proof = RangeProof::create(vec![55], vec![64], vec![&open], &mut transcript_create);

        assert!(TryInto::<pod::RangeProof128>::try_into(proof).is_err());
    }

    #[test]
    fn test_pod_range_proof_128() {
        let (comm_1, open_1) = Pedersen::commit(55_u64);
        let (comm_2, open_2) = Pedersen::commit(77_u64);
        let (comm_3, open_3) = Pedersen::commit(99_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::create(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        );

        let comm_1_point = comm_1.get_point().compress();
        let comm_2_point = comm_2.get_point().compress();
        let comm_3_point = comm_3.get_point().compress();

        let proof_serialized: pod::RangeProof128 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(
                vec![&comm_1_point, &comm_2_point, &comm_3_point],
                vec![64, 32, 32],
                &mut transcript_verify,
            )
            .is_ok());

        // should fail to serialize to pod::RangeProof64
        let proof = RangeProof::create(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        );

        assert!(TryInto::<pod::RangeProof64>::try_into(proof).is_err());
    }
}
