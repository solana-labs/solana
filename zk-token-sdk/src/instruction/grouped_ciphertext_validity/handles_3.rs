//! The grouped-ciphertext with 3 decryption handles validity proof instruction.
//!
//! A grouped-ciphertext validity proof certifies that a grouped ElGamal ciphertext is
//! well-defined, i.e. the ciphertext can be decrypted by private keys associated with its
//! decryption handles. To generate the proof, a prover must provide the Pedersen opening
//! associated with the grouped ciphertext's commitment.
//!
//! In accordance with the SPL Token program application, the first decryption handle associated
//! with the proof is referred to as the "source" handle, the second decryption handle is
//! referred to as the "destination" handle, and the third decryption handle is referred to as the
//! "auditor" handle.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalPubkey, grouped_elgamal::GroupedElGamalCiphertext,
            pedersen::PedersenOpening,
        },
        errors::{ProofGenerationError, ProofVerificationError},
        sigma_proofs::grouped_ciphertext_validity_proof::GroupedCiphertext3HandlesValidityProof,
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
/// `ProofInstruction::VerifyGroupedCiphertext3HandlesValidity` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct GroupedCiphertext3HandlesValidityProofData {
    pub context: GroupedCiphertext3HandlesValidityProofContext,

    pub proof: pod::GroupedCiphertext3HandlesValidityProof,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct GroupedCiphertext3HandlesValidityProofContext {
    pub source_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub auditor_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub grouped_ciphertext: pod::GroupedElGamalCiphertext3Handles, // 128 bytes
}

#[cfg(not(target_os = "solana"))]
impl GroupedCiphertext3HandlesValidityProofData {
    pub fn new(
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        grouped_ciphertext: &GroupedElGamalCiphertext<3>,
        amount: u64,
        opening: &PedersenOpening,
    ) -> Result<Self, ProofGenerationError> {
        let pod_source_pubkey = pod::ElGamalPubkey(source_pubkey.into());
        let pod_destination_pubkey = pod::ElGamalPubkey(destination_pubkey.into());
        let pod_auditor_pubkey = pod::ElGamalPubkey(auditor_pubkey.into());
        let pod_grouped_ciphertext = (*grouped_ciphertext).into();

        let context = GroupedCiphertext3HandlesValidityProofContext {
            source_pubkey: pod_source_pubkey,
            destination_pubkey: pod_destination_pubkey,
            auditor_pubkey: pod_auditor_pubkey,
            grouped_ciphertext: pod_grouped_ciphertext,
        };

        let mut transcript = context.new_transcript();

        let proof = GroupedCiphertext3HandlesValidityProof::new(
            source_pubkey,
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

impl ZkProofData<GroupedCiphertext3HandlesValidityProofContext>
    for GroupedCiphertext3HandlesValidityProofData
{
    const PROOF_TYPE: ProofType = ProofType::GroupedCiphertext3HandlesValidity;

    fn context_data(&self) -> &GroupedCiphertext3HandlesValidityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError> {
        let mut transcript = self.context.new_transcript();

        let source_pubkey = self.context.source_pubkey.try_into()?;
        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
        let grouped_ciphertext: GroupedElGamalCiphertext<3> =
            self.context.grouped_ciphertext.try_into()?;

        let source_handle = grouped_ciphertext.handles.first().unwrap();
        let destination_handle = grouped_ciphertext.handles.get(1).unwrap();
        let auditor_handle = grouped_ciphertext.handles.get(2).unwrap();

        let proof: GroupedCiphertext3HandlesValidityProof = self.proof.try_into()?;

        proof
            .verify(
                &grouped_ciphertext.commitment,
                &source_pubkey,
                &destination_pubkey,
                &auditor_pubkey,
                source_handle,
                destination_handle,
                auditor_handle,
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl GroupedCiphertext3HandlesValidityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"GroupedCiphertext3HandlesValidityProof");

        transcript.append_pubkey(b"source-pubkey", &self.source_pubkey);
        transcript.append_pubkey(b"destination-pubkey", &self.destination_pubkey);
        transcript.append_pubkey(b"auditor-pubkey", &self.auditor_pubkey);
        transcript
            .append_grouped_ciphertext_3_handles(b"grouped-ciphertext", &self.grouped_ciphertext);

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

        let amount: u64 = 55;
        let opening = PedersenOpening::new_rand();
        let grouped_ciphertext = GroupedElGamal::encrypt_with(
            [source_pubkey, destination_pubkey, auditor_pubkey],
            amount,
            &opening,
        );

        let proof_data = GroupedCiphertext3HandlesValidityProofData::new(
            source_pubkey,
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
