//! The ciphertext-ciphertext equality proof instruction.
//!
//! A ciphertext-ciphertext equality proof is defined with respect to two twisted ElGamal
//! ciphertexts. The proof certifies that the two ciphertexts encrypt the same message. To generate
//! the proof, a prover must provide the decryption key for the first ciphertext and the randomness
//! used to generate the second ciphertext.
//!
//! The first ciphertext associated with the proof is referred to as the "source" ciphertext. The
//! second ciphertext associated with the proof is referred to as the "destination" ciphertext.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        elgamal_program::errors::{ProofGenerationError, ProofVerificationError},
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::PedersenOpening,
        },
        sigma_proofs::ciphertext_ciphertext_equality::CiphertextCiphertextEqualityProof,
    },
    bytemuck::bytes_of,
    merlin::Transcript,
    std::convert::TryInto,
};
use {
    crate::{
        elgamal_program::proof_data::{ProofType, ZkProofData},
        encryption::pod::elgamal::{PodElGamalCiphertext, PodElGamalPubkey},
        sigma_proofs::pod::PodCiphertextCiphertextEqualityProof,
    },
    bytemuck::{Pod, Zeroable},
};

/// The instruction data that is needed for the
/// `ProofInstruction::VerifyCiphertextCiphertextEquality` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct CiphertextCiphertextEqualityProofData {
    pub context: CiphertextCiphertextEqualityProofContext,

    pub proof: PodCiphertextCiphertextEqualityProof,
}

/// The context data needed to verify a ciphertext-ciphertext equality proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct CiphertextCiphertextEqualityProofContext {
    pub source_pubkey: PodElGamalPubkey, // 32 bytes

    pub destination_pubkey: PodElGamalPubkey, // 32 bytes

    pub source_ciphertext: PodElGamalCiphertext, // 64 bytes

    pub destination_ciphertext: PodElGamalCiphertext, // 64 bytes
}

#[cfg(not(target_os = "solana"))]
impl CiphertextCiphertextEqualityProofData {
    pub fn new(
        source_keypair: &ElGamalKeypair,
        destination_pubkey: &ElGamalPubkey,
        source_ciphertext: &ElGamalCiphertext,
        destination_ciphertext: &ElGamalCiphertext,
        destination_opening: &PedersenOpening,
        amount: u64,
    ) -> Result<Self, ProofGenerationError> {
        let pod_source_pubkey = PodElGamalPubkey(source_keypair.pubkey().into());
        let pod_destination_pubkey = PodElGamalPubkey(destination_pubkey.into());
        let pod_source_ciphertext = PodElGamalCiphertext(source_ciphertext.to_bytes());
        let pod_destination_ciphertext = PodElGamalCiphertext(destination_ciphertext.to_bytes());

        let context = CiphertextCiphertextEqualityProofContext {
            source_pubkey: pod_source_pubkey,
            destination_pubkey: pod_destination_pubkey,
            source_ciphertext: pod_source_ciphertext,
            destination_ciphertext: pod_destination_ciphertext,
        };

        let mut transcript = context.new_transcript();

        let proof = CiphertextCiphertextEqualityProof::new(
            source_keypair,
            destination_pubkey,
            source_ciphertext,
            destination_opening,
            amount,
            &mut transcript,
        )
        .into();

        Ok(Self { context, proof })
    }
}

impl ZkProofData<CiphertextCiphertextEqualityProofContext>
    for CiphertextCiphertextEqualityProofData
{
    const PROOF_TYPE: ProofType = ProofType::CiphertextCiphertextEquality;

    fn context_data(&self) -> &CiphertextCiphertextEqualityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError> {
        let mut transcript = self.context.new_transcript();

        let source_pubkey = self.context.source_pubkey.try_into()?;
        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let source_ciphertext = self.context.source_ciphertext.try_into()?;
        let destination_ciphertext = self.context.destination_ciphertext.try_into()?;
        let proof: CiphertextCiphertextEqualityProof = self.proof.try_into()?;

        proof
            .verify(
                &source_pubkey,
                &destination_pubkey,
                &source_ciphertext,
                &destination_ciphertext,
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl CiphertextCiphertextEqualityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"ciphertext-ciphertext-equality-instruction");

        transcript.append_message(b"source-pubkey", bytes_of(&self.source_pubkey));
        transcript.append_message(b"destination-pubkey", bytes_of(&self.destination_pubkey));
        transcript.append_message(b"source-ciphertext", bytes_of(&self.source_ciphertext));
        transcript.append_message(
            b"destination-ciphertext",
            bytes_of(&self.destination_ciphertext),
        );

        transcript
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ciphertext_ciphertext_instruction_correctness() {
        let source_keypair = ElGamalKeypair::new_rand();
        let destination_keypair = ElGamalKeypair::new_rand();

        let amount: u64 = 0;
        let source_ciphertext = source_keypair.pubkey().encrypt(amount);

        let destination_opening = PedersenOpening::new_rand();
        let destination_ciphertext = destination_keypair
            .pubkey()
            .encrypt_with(amount, &destination_opening);

        let proof_data = CiphertextCiphertextEqualityProofData::new(
            &source_keypair,
            destination_keypair.pubkey(),
            &source_ciphertext,
            &destination_ciphertext,
            &destination_opening,
            amount,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());

        let amount: u64 = 55;
        let source_ciphertext = source_keypair.pubkey().encrypt(amount);

        let destination_opening = PedersenOpening::new_rand();
        let destination_ciphertext = destination_keypair
            .pubkey()
            .encrypt_with(amount, &destination_opening);

        let proof_data = CiphertextCiphertextEqualityProofData::new(
            &source_keypair,
            destination_keypair.pubkey(),
            &source_ciphertext,
            &destination_ciphertext,
            &destination_opening,
            amount,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());

        let amount = u64::MAX;
        let source_ciphertext = source_keypair.pubkey().encrypt(amount);

        let destination_opening = PedersenOpening::new_rand();
        let destination_ciphertext = destination_keypair
            .pubkey()
            .encrypt_with(amount, &destination_opening);

        let proof_data = CiphertextCiphertextEqualityProofData::new(
            &source_keypair,
            destination_keypair.pubkey(),
            &source_ciphertext,
            &destination_ciphertext,
            &destination_opening,
            amount,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());
    }
}
