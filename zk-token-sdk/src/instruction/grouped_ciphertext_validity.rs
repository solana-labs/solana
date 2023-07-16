//! The grouped-ciphertext validity proof instruction.
//!
//! A grouped-ciphertext validity proof certifies that a grouped ElGamal ciphertext is
//! well-defined, i.e. the ciphertext can be decrypted by private keys associated with its
//! decryption handles. To generate the proof, a prover must provide the Pedersen opening
//! associated with the grouped ciphertext's commitment.
//!
//! Currently, the grouped-ciphertext validity proof is restricted to ciphertexts with two handles.
//! In accordance with the SPL Token program application, the first decryption handle associated
//! with the proof is referred to as the "destination" handle and the second decryption handle is
//! referred to as the "auditor" handle.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalPubkey, grouped_elgamal::GroupedElGamalCiphertext,
            pedersen::PedersenOpening,
        },
        errors::ProofError,
        sigma_proofs::grouped_ciphertext_validity_proof::GroupedCiphertext2HandlesValidityProof,
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

/// The instruction data that is needed for the `ProofInstruction::VerifyGroupedCiphertextValidity`
/// instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct GroupedCiphertext2HandlesValidityProofData {
    pub context: GroupedCiphertext2HandlesValidityProofContext,

    pub proof: pod::GroupedCiphertext2HandlesValidityProof,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct GroupedCiphertext2HandlesValidityProofContext {
    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub auditor_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub grouped_ciphertext: pod::GroupedElGamalCiphertext2Handles, // 96 bytes
}

#[cfg(not(target_os = "solana"))]
impl GroupedCiphertext2HandlesValidityProofData {
    pub fn new(
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        grouped_ciphertext: &GroupedElGamalCiphertext<2>,
        amount: u64,
        opening: &PedersenOpening,
    ) -> Result<Self, ProofError> {
        let pod_destination_pubkey = pod::ElGamalPubkey(destination_pubkey.to_bytes());
        let pod_auditor_pubkey = pod::ElGamalPubkey(auditor_pubkey.to_bytes());
        let pod_grouped_ciphertext = (*grouped_ciphertext).into();

        let context = GroupedCiphertext2HandlesValidityProofContext {
            destination_pubkey: pod_destination_pubkey,
            auditor_pubkey: pod_auditor_pubkey,
            grouped_ciphertext: pod_grouped_ciphertext,
        };

        let mut transcript = context.new_transcript();

        let proof = GroupedCiphertext2HandlesValidityProof::new(
            (destination_pubkey, auditor_pubkey),
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
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context.new_transcript();

        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
        let grouped_ciphertext: GroupedElGamalCiphertext<2> =
            self.context.grouped_ciphertext.try_into()?;

        let destination_handle = grouped_ciphertext.handles.get(0).unwrap();
        let auditor_handle = grouped_ciphertext.handles.get(1).unwrap();

        let proof: GroupedCiphertext2HandlesValidityProof = self.proof.try_into()?;

        proof
            .verify(
                &grouped_ciphertext.commitment,
                (&destination_pubkey, &auditor_pubkey),
                (destination_handle, auditor_handle),
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl GroupedCiphertext2HandlesValidityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"CiphertextValidityProof");

        transcript.append_pubkey(b"destination-pubkey", &self.destination_pubkey);
        transcript.append_pubkey(b"auditor-pubkey", &self.auditor_pubkey);
        transcript
            .append_grouped_ciphertext_2_handles(b"grouped-ciphertext", &self.grouped_ciphertext);

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
