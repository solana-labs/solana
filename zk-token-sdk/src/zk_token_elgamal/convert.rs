use super::pod;
pub use target_arch::*;

impl From<(pod::PedersenCommitment, pod::DecryptHandle)> for pod::ElGamalCiphertext {
    fn from((comm, decrypt_handle): (pod::PedersenCommitment, pod::DecryptHandle)) -> Self {
        let mut buf = [0_u8; 64];
        buf[..32].copy_from_slice(&comm.0);
        buf[32..].copy_from_slice(&decrypt_handle.0);
        pod::ElGamalCiphertext(buf)
    }
}

#[cfg(not(target_arch = "bpf"))]
mod target_arch {
    use {
        super::pod,
        crate::{
            encryption::{
                auth_encryption::AeCiphertext,
                elgamal::{DecryptHandle, ElGamalCiphertext, ElGamalPubkey},
                pedersen::PedersenCommitment,
            },
            errors::ProofError,
            instruction::{
                transfer::{TransferAmountEncryption, TransferPubkeys},
                transfer_with_fee::{FeeEncryption, FeeParameters, TransferWithFeePubkeys},
            },
            range_proof::{errors::RangeProofError, RangeProof},
            sigma_proofs::{
                equality_proof::{CtxtCommEqualityProof, CtxtCtxtEqualityProof},
                errors::*,
                fee_proof::FeeSigmaProof,
                validity_proof::{AggregatedValidityProof, ValidityProof},
                zero_balance_proof::ZeroBalanceProof,
            },
        },
        curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar},
        std::convert::TryFrom,
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

    impl From<ElGamalCiphertext> for pod::ElGamalCiphertext {
        fn from(ct: ElGamalCiphertext) -> Self {
            Self(ct.to_bytes())
        }
    }

    impl TryFrom<pod::ElGamalCiphertext> for ElGamalCiphertext {
        type Error = ProofError;

        fn try_from(ct: pod::ElGamalCiphertext) -> Result<Self, Self::Error> {
            Self::from_bytes(&ct.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    impl From<ElGamalPubkey> for pod::ElGamalPubkey {
        fn from(pk: ElGamalPubkey) -> Self {
            Self(pk.to_bytes())
        }
    }

    impl TryFrom<pod::ElGamalPubkey> for ElGamalPubkey {
        type Error = ProofError;

        fn try_from(pk: pod::ElGamalPubkey) -> Result<Self, Self::Error> {
            Self::from_bytes(&pk.0).ok_or(ProofError::InconsistentCTData)
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

    impl From<PedersenCommitment> for pod::PedersenCommitment {
        fn from(comm: PedersenCommitment) -> Self {
            Self(comm.to_bytes())
        }
    }

    // For proof verification, interpret pod::PedersenComm directly as CompressedRistretto
    #[cfg(not(target_arch = "bpf"))]
    impl From<pod::PedersenCommitment> for CompressedRistretto {
        fn from(pod: pod::PedersenCommitment) -> Self {
            Self(pod.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl TryFrom<pod::PedersenCommitment> for PedersenCommitment {
        type Error = ProofError;

        fn try_from(pod: pod::PedersenCommitment) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl From<DecryptHandle> for pod::DecryptHandle {
        fn from(handle: DecryptHandle) -> Self {
            Self(handle.to_bytes())
        }
    }

    // For proof verification, interpret pod::PedersenDecHandle as CompressedRistretto
    #[cfg(not(target_arch = "bpf"))]
    impl From<pod::DecryptHandle> for CompressedRistretto {
        fn from(pod: pod::DecryptHandle) -> Self {
            Self(pod.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl TryFrom<pod::DecryptHandle> for DecryptHandle {
        type Error = ProofError;

        fn try_from(pod: pod::DecryptHandle) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    impl From<AeCiphertext> for pod::AeCiphertext {
        fn from(ct: AeCiphertext) -> Self {
            Self(ct.to_bytes())
        }
    }

    impl TryFrom<pod::AeCiphertext> for AeCiphertext {
        type Error = ProofError;

        fn try_from(ct: pod::AeCiphertext) -> Result<Self, Self::Error> {
            Self::from_bytes(&ct.0).ok_or(ProofError::InconsistentCTData)
        }
    }

    impl From<CtxtCommEqualityProof> for pod::CtxtCommEqualityProof {
        fn from(proof: CtxtCommEqualityProof) -> Self {
            Self(proof.to_bytes())
        }
    }

    impl TryFrom<pod::CtxtCommEqualityProof> for CtxtCommEqualityProof {
        type Error = EqualityProofError;

        fn try_from(pod: pod::CtxtCommEqualityProof) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    impl From<CtxtCtxtEqualityProof> for pod::CtxtCtxtEqualityProof {
        fn from(proof: CtxtCtxtEqualityProof) -> Self {
            Self(proof.to_bytes())
        }
    }

    impl TryFrom<pod::CtxtCtxtEqualityProof> for CtxtCtxtEqualityProof {
        type Error = EqualityProofError;

        fn try_from(pod: pod::CtxtCtxtEqualityProof) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    impl From<ValidityProof> for pod::ValidityProof {
        fn from(proof: ValidityProof) -> Self {
            Self(proof.to_bytes())
        }
    }

    impl TryFrom<pod::ValidityProof> for ValidityProof {
        type Error = ValidityProofError;

        fn try_from(pod: pod::ValidityProof) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    impl From<AggregatedValidityProof> for pod::AggregatedValidityProof {
        fn from(proof: AggregatedValidityProof) -> Self {
            Self(proof.to_bytes())
        }
    }

    impl TryFrom<pod::AggregatedValidityProof> for AggregatedValidityProof {
        type Error = ValidityProofError;

        fn try_from(pod: pod::AggregatedValidityProof) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    impl From<ZeroBalanceProof> for pod::ZeroBalanceProof {
        fn from(proof: ZeroBalanceProof) -> Self {
            Self(proof.to_bytes())
        }
    }

    impl TryFrom<pod::ZeroBalanceProof> for ZeroBalanceProof {
        type Error = ZeroBalanceProofError;

        fn try_from(pod: pod::ZeroBalanceProof) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    impl From<FeeSigmaProof> for pod::FeeSigmaProof {
        fn from(proof: FeeSigmaProof) -> Self {
            Self(proof.to_bytes())
        }
    }

    impl TryFrom<pod::FeeSigmaProof> for FeeSigmaProof {
        type Error = FeeSigmaProofError;

        fn try_from(pod: pod::FeeSigmaProof) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    impl TryFrom<RangeProof> for pod::RangeProof64 {
        type Error = RangeProofError;

        fn try_from(proof: RangeProof) -> Result<Self, Self::Error> {
            if proof.ipp_proof.serialized_size() != 448 {
                return Err(RangeProofError::Format);
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
        type Error = RangeProofError;

        fn try_from(pod: pod::RangeProof64) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl TryFrom<RangeProof> for pod::RangeProof128 {
        type Error = RangeProofError;

        fn try_from(proof: RangeProof) -> Result<Self, Self::Error> {
            if proof.ipp_proof.serialized_size() != 512 {
                return Err(RangeProofError::Format);
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
        type Error = RangeProofError;

        fn try_from(pod: pod::RangeProof128) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    impl TryFrom<RangeProof> for pod::RangeProof256 {
        type Error = RangeProofError;

        fn try_from(proof: RangeProof) -> Result<Self, Self::Error> {
            if proof.ipp_proof.serialized_size() != 576 {
                return Err(RangeProofError::Format);
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
            Ok(pod::RangeProof256(buf))
        }
    }

    impl TryFrom<pod::RangeProof256> for RangeProof {
        type Error = RangeProofError;

        fn try_from(pod: pod::RangeProof256) -> Result<Self, Self::Error> {
            Self::from_bytes(&pod.0)
        }
    }

    impl From<TransferPubkeys> for pod::TransferPubkeys {
        fn from(keys: TransferPubkeys) -> Self {
            Self {
                pubkey_source: keys.pubkey_source.into(),
                pubkey_dest: keys.pubkey_dest.into(),
                pubkey_auditor: keys.pubkey_auditor.into(),
            }
        }
    }

    impl TryFrom<pod::TransferPubkeys> for TransferPubkeys {
        type Error = ProofError;

        fn try_from(pod: pod::TransferPubkeys) -> Result<Self, Self::Error> {
            Ok(Self {
                pubkey_source: pod.pubkey_source.try_into()?,
                pubkey_dest: pod.pubkey_dest.try_into()?,
                pubkey_auditor: pod.pubkey_auditor.try_into()?,
            })
        }
    }

    impl From<TransferWithFeePubkeys> for pod::TransferWithFeePubkeys {
        fn from(keys: TransferWithFeePubkeys) -> Self {
            Self {
                pubkey_source: keys.pubkey_source.into(),
                pubkey_dest: keys.pubkey_dest.into(),
                pubkey_auditor: keys.pubkey_auditor.into(),
                pubkey_withdraw_withheld_authority: keys.pubkey_withdraw_withheld_authority.into(),
            }
        }
    }

    impl TryFrom<pod::TransferWithFeePubkeys> for TransferWithFeePubkeys {
        type Error = ProofError;

        fn try_from(pod: pod::TransferWithFeePubkeys) -> Result<Self, Self::Error> {
            Ok(Self {
                pubkey_source: pod.pubkey_source.try_into()?,
                pubkey_dest: pod.pubkey_dest.try_into()?,
                pubkey_auditor: pod.pubkey_auditor.try_into()?,
                pubkey_withdraw_withheld_authority: pod
                    .pubkey_withdraw_withheld_authority
                    .try_into()?,
            })
        }
    }

    impl From<TransferAmountEncryption> for pod::TransferAmountEncryption {
        fn from(ciphertext: TransferAmountEncryption) -> Self {
            Self {
                commitment: ciphertext.commitment.into(),
                handle_source: ciphertext.handle_source.into(),
                handle_dest: ciphertext.handle_dest.into(),
                handle_auditor: ciphertext.handle_auditor.into(),
            }
        }
    }

    impl TryFrom<pod::TransferAmountEncryption> for TransferAmountEncryption {
        type Error = ProofError;

        fn try_from(pod: pod::TransferAmountEncryption) -> Result<Self, Self::Error> {
            Ok(Self {
                commitment: pod.commitment.try_into()?,
                handle_source: pod.handle_source.try_into()?,
                handle_dest: pod.handle_dest.try_into()?,
                handle_auditor: pod.handle_auditor.try_into()?,
            })
        }
    }

    impl From<FeeEncryption> for pod::FeeEncryption {
        fn from(ciphertext: FeeEncryption) -> Self {
            Self {
                commitment: ciphertext.commitment.into(),
                handle_dest: ciphertext.handle_dest.into(),
                handle_withdraw_withheld_authority: ciphertext
                    .handle_withdraw_withheld_authority
                    .into(),
            }
        }
    }

    impl TryFrom<pod::FeeEncryption> for FeeEncryption {
        type Error = ProofError;

        fn try_from(pod: pod::FeeEncryption) -> Result<Self, Self::Error> {
            Ok(Self {
                commitment: pod.commitment.try_into()?,
                handle_dest: pod.handle_dest.try_into()?,
                handle_withdraw_withheld_authority: pod
                    .handle_withdraw_withheld_authority
                    .try_into()?,
            })
        }
    }

    impl From<FeeParameters> for pod::FeeParameters {
        fn from(parameters: FeeParameters) -> Self {
            Self {
                fee_rate_basis_points: parameters.fee_rate_basis_points.into(),
                maximum_fee: parameters.maximum_fee.into(),
            }
        }
    }

    impl From<pod::FeeParameters> for FeeParameters {
        fn from(pod: pod::FeeParameters) -> Self {
            Self {
                fee_rate_basis_points: pod.fee_rate_basis_points.into(),
                maximum_fee: pod.maximum_fee.into(),
            }
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
        let (comm, open) = Pedersen::new(55_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::new(vec![55], vec![64], vec![&open], &mut transcript_create);

        let proof_serialized: pod::RangeProof64 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(vec![&comm], vec![64], &mut transcript_verify)
            .is_ok());

        // should fail to serialize to pod::RangeProof128
        let proof = RangeProof::new(vec![55], vec![64], vec![&open], &mut transcript_create);

        assert!(TryInto::<pod::RangeProof128>::try_into(proof).is_err());
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

        let proof_serialized: pod::RangeProof128 = proof.try_into().unwrap();
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
        );

        assert!(TryInto::<pod::RangeProof64>::try_into(proof).is_err());
    }
}
