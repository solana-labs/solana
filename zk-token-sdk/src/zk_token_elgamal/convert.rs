pub use target_arch::*;
use {super::pod, crate::curve25519::ristretto::PodRistrettoPoint};

impl From<(pod::PedersenCommitment, pod::DecryptHandle)> for pod::ElGamalCiphertext {
    fn from((commitment, handle): (pod::PedersenCommitment, pod::DecryptHandle)) -> Self {
        let mut buf = [0_u8; 64];
        buf[..32].copy_from_slice(&commitment.0);
        buf[32..].copy_from_slice(&handle.0);
        pod::ElGamalCiphertext(buf)
    }
}

impl From<pod::ElGamalCiphertext> for (pod::PedersenCommitment, pod::DecryptHandle) {
    fn from(ciphertext: pod::ElGamalCiphertext) -> Self {
        let commitment: [u8; 32] = ciphertext.0[..32].try_into().unwrap();
        let handle: [u8; 32] = ciphertext.0[32..].try_into().unwrap();

        (
            pod::PedersenCommitment(commitment),
            pod::DecryptHandle(handle),
        )
    }
}

impl From<pod::PedersenCommitment> for PodRistrettoPoint {
    fn from(commitment: pod::PedersenCommitment) -> Self {
        PodRistrettoPoint(commitment.0)
    }
}

impl From<PodRistrettoPoint> for pod::PedersenCommitment {
    fn from(point: PodRistrettoPoint) -> Self {
        pod::PedersenCommitment(point.0)
    }
}

impl From<pod::DecryptHandle> for PodRistrettoPoint {
    fn from(handle: pod::DecryptHandle) -> Self {
        PodRistrettoPoint(handle.0)
    }
}

impl From<PodRistrettoPoint> for pod::DecryptHandle {
    fn from(point: PodRistrettoPoint) -> Self {
        pod::DecryptHandle(point.0)
    }
}

#[cfg(not(target_os = "solana"))]
mod target_arch {
    use {
        super::pod,
        crate::{curve25519::scalar::PodScalar, errors::ProofError},
        curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar},
        std::convert::TryFrom,
    };

    impl From<Scalar> for PodScalar {
        fn from(scalar: Scalar) -> Self {
            Self(scalar.to_bytes())
        }
    }

    impl TryFrom<PodScalar> for Scalar {
        type Error = ProofError;

        fn try_from(pod: PodScalar) -> Result<Self, Self::Error> {
            Scalar::from_canonical_bytes(pod.0).ok_or(ProofError::CiphertextDeserialization)
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
}

#[cfg(target_os = "solana")]
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
        let (comm, open) = Pedersen::new(55_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::new(vec![55], vec![64], vec![&open], &mut transcript_create);

        let proof_serialized: pod::RangeProofU64 = proof.unwrap().try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(vec![&comm], vec![64], &mut transcript_verify)
            .is_ok());

        // should fail to serialize to pod::RangeProof128
        let proof =
            RangeProof::new(vec![55], vec![64], vec![&open], &mut transcript_create).unwrap();

        assert!(TryInto::<pod::RangeProofU128>::try_into(proof).is_err());
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
        )
        .unwrap();

        let proof_serialized: pod::RangeProofU128 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(
                vec![&comm_1, &comm_2, &comm_3],
                vec![64, 32, 32],
                &mut transcript_verify,
            )
            .is_ok());

        // should fail to serialize to pod::RangeProof64
        let proof = RangeProof::new(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        )
        .unwrap();

        assert!(TryInto::<pod::RangeProofU64>::try_into(proof).is_err());
    }
}
