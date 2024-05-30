//! The grouped-ciphertext validity proof instruction.
//!
//! A grouped-ciphertext validity proof certifies that a grouped ElGamal ciphertext is
//! well-defined, i.e. the ciphertext can be decrypted by private keys associated with its
//! decryption handles. To generate the proof, a prover must provide the Pedersen opening
//! associated with the grouped ciphertext's commitment.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        elgamal_program::errors::{ProofGenerationError, ProofVerificationError},
        encryption::{
            elgamal::ElGamalPubkey, grouped_elgamal::GroupedElGamalCiphertext,
            pedersen::PedersenOpening,
        },
        sigma_proofs::grouped_ciphertext_validity::GroupedCiphertext2HandlesValidityProof,
    },
    bytemuck::bytes_of,
    merlin::Transcript,
};
use {
    crate::{
        elgamal_program::proof_data::{ProofType, ZkProofData},
        encryption::pod::{
            elgamal::PodElGamalPubkey, grouped_elgamal::PodGroupedElGamalCiphertext2Handles,
        },
        sigma_proofs::pod::PodGroupedCiphertext2HandlesValidityProof,
    },
    bytemuck::{Pod, Zeroable},
};

/// The instruction data that is needed for the `ProofInstruction::VerifyGroupedCiphertextValidity`
/// instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct GroupedCiphertext2HandlesValidityProofData {
    pub context: GroupedCiphertext2HandlesValidityProofContext,

    pub proof: PodGroupedCiphertext2HandlesValidityProof,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct GroupedCiphertext2HandlesValidityProofContext {
    pub destination_pubkey: PodElGamalPubkey, // 32 bytes

    pub auditor_pubkey: PodElGamalPubkey, // 32 bytes

    pub grouped_ciphertext: PodGroupedElGamalCiphertext2Handles, // 96 bytes
}

#[cfg(not(target_os = "solana"))]
impl GroupedCiphertext2HandlesValidityProofData {
    pub fn new(
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        grouped_ciphertext: &GroupedElGamalCiphertext<2>,
        amount: u64,
        opening: &PedersenOpening,
    ) -> Result<Self, ProofGenerationError> {
        let pod_destination_pubkey = PodElGamalPubkey(destination_pubkey.into());
        let pod_auditor_pubkey = PodElGamalPubkey(auditor_pubkey.into());
        let pod_grouped_ciphertext = (*grouped_ciphertext).into();

        let context = GroupedCiphertext2HandlesValidityProofContext {
            destination_pubkey: pod_destination_pubkey,
            auditor_pubkey: pod_auditor_pubkey,
            grouped_ciphertext: pod_grouped_ciphertext,
        };

        let mut transcript = context.new_transcript();

        let proof = GroupedCiphertext2HandlesValidityProof::new(
            destination_pubkey,
            auditor_pubkey,
            amount,
            opening,
            &mut transcript,
        )
        .into();

        Ok(Self { context, proof })
    }
}

impl ZkProofData<GroupedCiphertext2HandlesValidityProofContext>
    for GroupedCiphertext2HandlesValidityProofData
{
    const PROOF_TYPE: ProofType = ProofType::GroupedCiphertext2HandlesValidity;

    fn context_data(&self) -> &GroupedCiphertext2HandlesValidityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError> {
        let mut transcript = self.context.new_transcript();

        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
        let grouped_ciphertext: GroupedElGamalCiphertext<2> =
            self.context.grouped_ciphertext.try_into()?;

        let destination_handle = grouped_ciphertext.handles.first().unwrap();
        let auditor_handle = grouped_ciphertext.handles.get(1).unwrap();

        let proof: GroupedCiphertext2HandlesValidityProof = self.proof.try_into()?;

        proof
            .verify(
                &grouped_ciphertext.commitment,
                &destination_pubkey,
                &auditor_pubkey,
                destination_handle,
                auditor_handle,
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl GroupedCiphertext2HandlesValidityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"grouped-ciphertext-validity-2-handles-instruction");

        transcript.append_message(b"destination-pubkey", bytes_of(&self.destination_pubkey));
        transcript.append_message(b"auditor-pubkey", bytes_of(&self.auditor_pubkey));
        transcript.append_message(b"grouped-ciphertext", bytes_of(&self.grouped_ciphertext));

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

        let amount: u64 = 55;
        let opening = PedersenOpening::new_rand();
        let grouped_ciphertext =
            GroupedElGamal::encrypt_with([destination_pubkey, auditor_pubkey], amount, &opening);

        let proof_data = GroupedCiphertext2HandlesValidityProofData::new(
            destination_pubkey,
            auditor_pubkey,
            &grouped_ciphertext,
            amount,
            &opening,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());
    }
}
