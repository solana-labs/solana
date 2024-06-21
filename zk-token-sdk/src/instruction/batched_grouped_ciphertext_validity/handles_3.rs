//! The batched grouped-ciphertext with 3 handles validity proof instruction.
//!
//! A batched grouped-ciphertext validity proof certifies the validity of two grouped ElGamal
//! ciphertext that are encrypted using the same set of ElGamal public keys. A batched
//! grouped-ciphertext validity proof is shorter and more efficient than two individual
//! grouped-ciphertext validity proofs.
//!
//! In accordance with the SPL Token program application, the first decryption handle associated
//! with the proof is referred to as the "source" handle, the second decryption handle is referred
//! to as the "destination" handle, and the third decryption handle is referred to as the "auditor"
//! handle. Furthermore, the first grouped ciphertext is referred to as the "lo" ciphertext and the
//! second grouped ciphertext is referred to as the "hi" ciphertext.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalPubkey, grouped_elgamal::GroupedElGamalCiphertext,
            pedersen::PedersenOpening,
        },
        errors::{ProofGenerationError, ProofVerificationError},
        sigma_proofs::batched_grouped_ciphertext_validity_proof::BatchedGroupedCiphertext3HandlesValidityProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
};
use {
    crate::{
        instruction::{ProofType, ZkProofData},
        zk_token_elgamal::pod,
    },
    bytemuck_derive::{Pod, Zeroable},
};

/// The instruction data that is needed for the
/// `ProofInstruction::VerifyBatchedGroupedCiphertext3HandlesValidity` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct BatchedGroupedCiphertext3HandlesValidityProofData {
    pub context: BatchedGroupedCiphertext3HandlesValidityProofContext,

    pub proof: pod::BatchedGroupedCiphertext3HandlesValidityProof,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct BatchedGroupedCiphertext3HandlesValidityProofContext {
    pub source_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub auditor_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub grouped_ciphertext_lo: pod::GroupedElGamalCiphertext3Handles, // 128 bytes

    pub grouped_ciphertext_hi: pod::GroupedElGamalCiphertext3Handles, // 128 bytes
}

#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext3HandlesValidityProofData {
    pub fn new(
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        grouped_ciphertext_lo: &GroupedElGamalCiphertext<3>,
        grouped_ciphertext_hi: &GroupedElGamalCiphertext<3>,
        amount_lo: u64,
        amount_hi: u64,
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
    ) -> Result<Self, ProofGenerationError> {
        let pod_source_pubkey = pod::ElGamalPubkey(source_pubkey.into());
        let pod_destination_pubkey = pod::ElGamalPubkey(destination_pubkey.into());
        let pod_auditor_pubkey = pod::ElGamalPubkey(auditor_pubkey.into());
        let pod_grouped_ciphertext_lo = (*grouped_ciphertext_lo).into();
        let pod_grouped_ciphertext_hi = (*grouped_ciphertext_hi).into();

        let context = BatchedGroupedCiphertext3HandlesValidityProofContext {
            source_pubkey: pod_source_pubkey,
            destination_pubkey: pod_destination_pubkey,
            auditor_pubkey: pod_auditor_pubkey,
            grouped_ciphertext_lo: pod_grouped_ciphertext_lo,
            grouped_ciphertext_hi: pod_grouped_ciphertext_hi,
        };

        let mut transcript = context.new_transcript();

        let proof = BatchedGroupedCiphertext3HandlesValidityProof::new(
            source_pubkey,
            destination_pubkey,
            auditor_pubkey,
            amount_lo,
            amount_hi,
            opening_lo,
            opening_hi,
            &mut transcript,
        )
        .into();

        Ok(Self { context, proof })
    }
}

impl ZkProofData<BatchedGroupedCiphertext3HandlesValidityProofContext>
    for BatchedGroupedCiphertext3HandlesValidityProofData
{
    const PROOF_TYPE: ProofType = ProofType::BatchedGroupedCiphertext3HandlesValidity;

    fn context_data(&self) -> &BatchedGroupedCiphertext3HandlesValidityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError> {
        let mut transcript = self.context.new_transcript();

        let source_pubkey = self.context.source_pubkey.try_into()?;
        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
        let grouped_ciphertext_lo: GroupedElGamalCiphertext<3> =
            self.context.grouped_ciphertext_lo.try_into()?;
        let grouped_ciphertext_hi: GroupedElGamalCiphertext<3> =
            self.context.grouped_ciphertext_hi.try_into()?;

        let source_handle_lo = grouped_ciphertext_lo.handles.first().unwrap();
        let destination_handle_lo = grouped_ciphertext_lo.handles.get(1).unwrap();
        let auditor_handle_lo = grouped_ciphertext_lo.handles.get(2).unwrap();

        let source_handle_hi = grouped_ciphertext_hi.handles.first().unwrap();
        let destination_handle_hi = grouped_ciphertext_hi.handles.get(1).unwrap();
        let auditor_handle_hi = grouped_ciphertext_hi.handles.get(2).unwrap();

        let proof: BatchedGroupedCiphertext3HandlesValidityProof = self.proof.try_into()?;

        proof
            .verify(
                &source_pubkey,
                &destination_pubkey,
                &auditor_pubkey,
                &grouped_ciphertext_lo.commitment,
                &grouped_ciphertext_hi.commitment,
                source_handle_lo,
                source_handle_hi,
                destination_handle_lo,
                destination_handle_hi,
                auditor_handle_lo,
                auditor_handle_hi,
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext3HandlesValidityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"BatchedGroupedCiphertext3HandlesValidityProof");

        transcript.append_pubkey(b"source-pubkey", &self.source_pubkey);
        transcript.append_pubkey(b"destination-pubkey", &self.destination_pubkey);
        transcript.append_pubkey(b"auditor-pubkey", &self.auditor_pubkey);
        transcript.append_grouped_ciphertext_3_handles(
            b"grouped-ciphertext-lo",
            &self.grouped_ciphertext_lo,
        );
        transcript.append_grouped_ciphertext_3_handles(
            b"grouped-ciphertext-hi",
            &self.grouped_ciphertext_hi,
        );

        transcript
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{elgamal::ElGamalKeypair, grouped_elgamal::GroupedElGamal},
    };

    #[test]
    fn test_ciphertext_validity_proof_instruction_correctness() {
        let source_keypair = ElGamalKeypair::new_rand();
        let source_pubkey = source_keypair.pubkey();

        let destination_keypair = ElGamalKeypair::new_rand();
        let destination_pubkey = destination_keypair.pubkey();

        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pubkey = auditor_keypair.pubkey();

        let amount_lo: u64 = 11;
        let amount_hi: u64 = 22;

        let opening_lo = PedersenOpening::new_rand();
        let opening_hi = PedersenOpening::new_rand();

        let grouped_ciphertext_lo = GroupedElGamal::encrypt_with(
            [source_pubkey, destination_pubkey, auditor_pubkey],
            amount_lo,
            &opening_lo,
        );

        let grouped_ciphertext_hi = GroupedElGamal::encrypt_with(
            [source_pubkey, destination_pubkey, auditor_pubkey],
            amount_hi,
            &opening_hi,
        );

        let proof_data = BatchedGroupedCiphertext3HandlesValidityProofData::new(
            source_pubkey,
            destination_pubkey,
            auditor_pubkey,
            &grouped_ciphertext_lo,
            &grouped_ciphertext_hi,
            amount_lo,
            amount_hi,
            &opening_lo,
            &opening_hi,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());
    }
}
