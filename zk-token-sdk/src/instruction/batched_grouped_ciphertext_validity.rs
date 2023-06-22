//! The batched grouped-ciphertext validity proof instruction.
//!
//! A batched grouped-ciphertext validity proof certifies the validity of two grouped ElGamal
//! ciphertext that are encrypted using the same set of ElGamal public keys. A batched
//! grouped-ciphertext validity proof is shorter and more efficient than two individual
//! grouped-ciphertext validity proofs.
//!
//! Currently, the batched grouped-ciphertext validity proof is restricted to ciphertexts with two
//! handles. In accordance with the SPL Token program application, the first decryption handle
//! associated with the proof is referred to as the "destination" handle and the second decryption
//! handle is referred to as the "auditor" handle. Furthermore, the first grouped ciphertext is
//! referred to as the "lo" ciphertext and the second grouped ciphertext is referred to as the "hi"
//! ciphertext.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalPubkey, grouped_elgamal::GroupedElGamalCiphertext,
            pedersen::PedersenOpening,
        },
        errors::ProofError,
        sigma_proofs::batched_grouped_ciphertext_validity_proof::BatchedGroupedCiphertext2HandlesValidityProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
};
use {
    crate::{
        instruction::{ProofType, ZkProofData},
        zk_token_elgamal::pod,
    },
    bytemuck::{Pod, Zeroable},
};

/// The instruction data that is needed for the
/// `ProofInstruction::VerifyBatchedGroupedCiphertextValidity` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct BatchedGroupedCiphertext2HandlesValidityProofData {
    pub context: BatchedGroupedCiphertext2HandlesValidityProofContext,

    pub proof: pod::BatchedGroupedCiphertext2HandlesValidityProof,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct BatchedGroupedCiphertext2HandlesValidityProofContext {
    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub auditor_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub grouped_ciphertext_lo: pod::GroupedElGamalCiphertext2Handles, // 96 bytes

    pub grouped_ciphertext_hi: pod::GroupedElGamalCiphertext2Handles, // 96 bytes
}

#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext2HandlesValidityProofData {
    pub fn new(
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        grouped_ciphertext_lo: &GroupedElGamalCiphertext<2>,
        grouped_ciphertext_hi: &GroupedElGamalCiphertext<2>,
        amount_lo: u64,
        amount_hi: u64,
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
    ) -> Result<Self, ProofError> {
        let pod_destination_pubkey = pod::ElGamalPubkey(destination_pubkey.to_bytes());
        let pod_auditor_pubkey = pod::ElGamalPubkey(auditor_pubkey.to_bytes());
        let pod_grouped_ciphertext_lo = (*grouped_ciphertext_lo).into();
        let pod_grouped_ciphertext_hi = (*grouped_ciphertext_hi).into();

        let context = BatchedGroupedCiphertext2HandlesValidityProofContext {
            destination_pubkey: pod_destination_pubkey,
            auditor_pubkey: pod_auditor_pubkey,
            grouped_ciphertext_lo: pod_grouped_ciphertext_lo,
            grouped_ciphertext_hi: pod_grouped_ciphertext_hi,
        };

        let mut transcript = context.new_transcript();

        let proof = BatchedGroupedCiphertext2HandlesValidityProof::new(
            (destination_pubkey, auditor_pubkey),
            (amount_lo, amount_hi),
            (opening_lo, opening_hi),
            &mut transcript,
        )
        .into();

        Ok(Self { context, proof })
    }
}

impl ZkProofData<BatchedGroupedCiphertext2HandlesValidityProofContext>
    for BatchedGroupedCiphertext2HandlesValidityProofData
{
    const PROOF_TYPE: ProofType = ProofType::BatchedGroupedCiphertext2HandlesValidity;

    fn context_data(&self) -> &BatchedGroupedCiphertext2HandlesValidityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context.new_transcript();

        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
        let grouped_ciphertext_lo: GroupedElGamalCiphertext<2> =
            self.context.grouped_ciphertext_lo.try_into()?;
        let grouped_ciphertext_hi: GroupedElGamalCiphertext<2> =
            self.context.grouped_ciphertext_hi.try_into()?;

        let destination_handle_lo = grouped_ciphertext_lo.handles.get(0).unwrap();
        let auditor_handle_lo = grouped_ciphertext_lo.handles.get(1).unwrap();

        let destination_handle_hi = grouped_ciphertext_hi.handles.get(0).unwrap();
        let auditor_handle_hi = grouped_ciphertext_hi.handles.get(1).unwrap();

        let proof: BatchedGroupedCiphertext2HandlesValidityProof = self.proof.try_into()?;

        proof
            .verify(
                (&destination_pubkey, &auditor_pubkey),
                (
                    &grouped_ciphertext_lo.commitment,
                    &grouped_ciphertext_hi.commitment,
                ),
                (destination_handle_lo, destination_handle_hi),
                (auditor_handle_lo, auditor_handle_hi),
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext2HandlesValidityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"BatchedGroupedCiphertextValidityProof");

        transcript.append_pubkey(b"destination-pubkey", &self.destination_pubkey);
        transcript.append_pubkey(b"auditor-pubkey", &self.auditor_pubkey);
        transcript.append_grouped_ciphertext_2_handles(
            b"grouped-ciphertext-lo",
            &self.grouped_ciphertext_lo,
        );
        transcript.append_grouped_ciphertext_2_handles(
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
        let destination_keypair = ElGamalKeypair::new_rand();
        let destination_pubkey = destination_keypair.pubkey();

        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pubkey = auditor_keypair.pubkey();

        let amount_lo: u64 = 11;
        let amount_hi: u64 = 22;

        let opening_lo = PedersenOpening::new_rand();
        let opening_hi = PedersenOpening::new_rand();

        let grouped_ciphertext_lo = GroupedElGamal::encrypt_with(
            [destination_pubkey, auditor_pubkey],
            amount_lo,
            &opening_lo,
        );

        let grouped_ciphertext_hi = GroupedElGamal::encrypt_with(
            [destination_pubkey, auditor_pubkey],
            amount_hi,
            &opening_hi,
        );

        let proof_data = BatchedGroupedCiphertext2HandlesValidityProofData::new(
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
