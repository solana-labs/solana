//! The batched grouped-ciphertext with 3 handles validity proof instruction.
//!
//! A batched grouped-ciphertext validity proof certifies the validity of two grouped ElGamal
//! ciphertext that are encrypted using the same set of ElGamal public keys. A batched
//! grouped-ciphertext validity proof is shorter and more efficient than two individual
//! grouped-ciphertext validity proofs.

use {
    crate::{
        encryption::pod::{
            elgamal::PodElGamalPubkey, grouped_elgamal::PodGroupedElGamalCiphertext3Handles,
        },
        sigma_proofs::pod::PodBatchedGroupedCiphertext3HandlesValidityProof,
        zk_elgamal_proof_program::proof_data::{ProofType, ZkProofData},
    },
    bytemuck_derive::{Pod, Zeroable},
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalPubkey, grouped_elgamal::GroupedElGamalCiphertext,
            pedersen::PedersenOpening,
        },
        sigma_proofs::batched_grouped_ciphertext_validity::BatchedGroupedCiphertext3HandlesValidityProof,
        zk_elgamal_proof_program::errors::{ProofGenerationError, ProofVerificationError},
    },
    bytemuck::bytes_of,
    merlin::Transcript,
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

    pub proof: PodBatchedGroupedCiphertext3HandlesValidityProof,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct BatchedGroupedCiphertext3HandlesValidityProofContext {
    pub first_pubkey: PodElGamalPubkey, // 32 bytes

    pub second_pubkey: PodElGamalPubkey, // 32 bytes

    pub third_pubkey: PodElGamalPubkey, // 32 bytes

    pub grouped_ciphertext_lo: PodGroupedElGamalCiphertext3Handles, // 128 bytes

    pub grouped_ciphertext_hi: PodGroupedElGamalCiphertext3Handles, // 128 bytes
}

#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext3HandlesValidityProofData {
    pub fn new(
        first_pubkey: &ElGamalPubkey,
        second_pubkey: &ElGamalPubkey,
        third_pubkey: &ElGamalPubkey,
        grouped_ciphertext_lo: &GroupedElGamalCiphertext<3>,
        grouped_ciphertext_hi: &GroupedElGamalCiphertext<3>,
        amount_lo: u64,
        amount_hi: u64,
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
    ) -> Result<Self, ProofGenerationError> {
        let pod_first_pubkey = PodElGamalPubkey(first_pubkey.into());
        let pod_second_pubkey = PodElGamalPubkey(second_pubkey.into());
        let pod_third_pubkey = PodElGamalPubkey(third_pubkey.into());
        let pod_grouped_ciphertext_lo = (*grouped_ciphertext_lo).into();
        let pod_grouped_ciphertext_hi = (*grouped_ciphertext_hi).into();

        let context = BatchedGroupedCiphertext3HandlesValidityProofContext {
            first_pubkey: pod_first_pubkey,
            second_pubkey: pod_second_pubkey,
            third_pubkey: pod_third_pubkey,
            grouped_ciphertext_lo: pod_grouped_ciphertext_lo,
            grouped_ciphertext_hi: pod_grouped_ciphertext_hi,
        };

        let mut transcript = context.new_transcript();

        let proof = BatchedGroupedCiphertext3HandlesValidityProof::new(
            first_pubkey,
            second_pubkey,
            third_pubkey,
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

        let first_pubkey = self.context.first_pubkey.try_into()?;
        let second_pubkey = self.context.second_pubkey.try_into()?;
        let third_pubkey = self.context.third_pubkey.try_into()?;
        let grouped_ciphertext_lo: GroupedElGamalCiphertext<3> =
            self.context.grouped_ciphertext_lo.try_into()?;
        let grouped_ciphertext_hi: GroupedElGamalCiphertext<3> =
            self.context.grouped_ciphertext_hi.try_into()?;

        let first_handle_lo = grouped_ciphertext_lo.handles.first().unwrap();
        let second_handle_lo = grouped_ciphertext_lo.handles.get(1).unwrap();
        let third_handle_lo = grouped_ciphertext_lo.handles.get(2).unwrap();

        let first_handle_hi = grouped_ciphertext_hi.handles.first().unwrap();
        let second_handle_hi = grouped_ciphertext_hi.handles.get(1).unwrap();
        let third_handle_hi = grouped_ciphertext_hi.handles.get(2).unwrap();

        let proof: BatchedGroupedCiphertext3HandlesValidityProof = self.proof.try_into()?;

        proof
            .verify(
                &first_pubkey,
                &second_pubkey,
                &third_pubkey,
                &grouped_ciphertext_lo.commitment,
                &grouped_ciphertext_hi.commitment,
                first_handle_lo,
                first_handle_hi,
                second_handle_lo,
                second_handle_hi,
                third_handle_lo,
                third_handle_hi,
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext3HandlesValidityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript =
            Transcript::new(b"batched-grouped-ciphertext-validity-3-handles-instruction");

        transcript.append_message(b"first-pubkey", bytes_of(&self.first_pubkey));
        transcript.append_message(b"second-pubkey", bytes_of(&self.second_pubkey));
        transcript.append_message(b"third-pubkey", bytes_of(&self.third_pubkey));
        transcript.append_message(
            b"grouped-ciphertext-lo",
            bytes_of(&self.grouped_ciphertext_lo),
        );
        transcript.append_message(
            b"grouped-ciphertext-hi",
            bytes_of(&self.grouped_ciphertext_hi),
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
        let first_keypair = ElGamalKeypair::new_rand();
        let first_pubkey = first_keypair.pubkey();

        let second_keypair = ElGamalKeypair::new_rand();
        let second_pubkey = second_keypair.pubkey();

        let third_keypair = ElGamalKeypair::new_rand();
        let third_pubkey = third_keypair.pubkey();

        let amount_lo: u64 = 11;
        let amount_hi: u64 = 22;

        let opening_lo = PedersenOpening::new_rand();
        let opening_hi = PedersenOpening::new_rand();

        let grouped_ciphertext_lo = GroupedElGamal::encrypt_with(
            [first_pubkey, second_pubkey, third_pubkey],
            amount_lo,
            &opening_lo,
        );

        let grouped_ciphertext_hi = GroupedElGamal::encrypt_with(
            [first_pubkey, second_pubkey, third_pubkey],
            amount_hi,
            &opening_hi,
        );

        let proof_data = BatchedGroupedCiphertext3HandlesValidityProofData::new(
            first_pubkey,
            second_pubkey,
            third_pubkey,
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
