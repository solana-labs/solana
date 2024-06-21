//! The ciphertext-ciphertext equality proof instruction.
//!
//! A ciphertext-ciphertext equality proof is defined with respect to two twisted ElGamal
//! ciphertexts. The proof certifies that the two ciphertexts encrypt the same message. To generate
//! the proof, a prover must provide the decryption key for the first ciphertext and the randomness
//! used to generate the second ciphertext.

use {
    crate::{
        encryption::pod::elgamal::{PodElGamalCiphertext, PodElGamalPubkey},
        sigma_proofs::pod::PodCiphertextCiphertextEqualityProof,
        zk_elgamal_proof_program::proof_data::{ProofType, ZkProofData},
    },
    bytemuck_derive::{Pod, Zeroable},
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::PedersenOpening,
        },
        sigma_proofs::ciphertext_ciphertext_equality::CiphertextCiphertextEqualityProof,
        zk_elgamal_proof_program::errors::{ProofGenerationError, ProofVerificationError},
    },
    bytemuck::bytes_of,
    merlin::Transcript,
    std::convert::TryInto,
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
    pub first_pubkey: PodElGamalPubkey, // 32 bytes

    pub second_pubkey: PodElGamalPubkey, // 32 bytes

    pub first_ciphertext: PodElGamalCiphertext, // 64 bytes

    pub second_ciphertext: PodElGamalCiphertext, // 64 bytes
}

#[cfg(not(target_os = "solana"))]
impl CiphertextCiphertextEqualityProofData {
    pub fn new(
        first_keypair: &ElGamalKeypair,
        second_pubkey: &ElGamalPubkey,
        first_ciphertext: &ElGamalCiphertext,
        second_ciphertext: &ElGamalCiphertext,
        second_opening: &PedersenOpening,
        amount: u64,
    ) -> Result<Self, ProofGenerationError> {
        let pod_first_pubkey = PodElGamalPubkey(first_keypair.pubkey().into());
        let pod_second_pubkey = PodElGamalPubkey(second_pubkey.into());
        let pod_first_ciphertext = PodElGamalCiphertext(first_ciphertext.to_bytes());
        let pod_second_ciphertext = PodElGamalCiphertext(second_ciphertext.to_bytes());

        let context = CiphertextCiphertextEqualityProofContext {
            first_pubkey: pod_first_pubkey,
            second_pubkey: pod_second_pubkey,
            first_ciphertext: pod_first_ciphertext,
            second_ciphertext: pod_second_ciphertext,
        };

        let mut transcript = context.new_transcript();

        let proof = CiphertextCiphertextEqualityProof::new(
            first_keypair,
            second_pubkey,
            first_ciphertext,
            second_opening,
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

        let first_pubkey = self.context.first_pubkey.try_into()?;
        let second_pubkey = self.context.second_pubkey.try_into()?;
        let first_ciphertext = self.context.first_ciphertext.try_into()?;
        let second_ciphertext = self.context.second_ciphertext.try_into()?;
        let proof: CiphertextCiphertextEqualityProof = self.proof.try_into()?;

        proof
            .verify(
                &first_pubkey,
                &second_pubkey,
                &first_ciphertext,
                &second_ciphertext,
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

        transcript.append_message(b"first-pubkey", bytes_of(&self.first_pubkey));
        transcript.append_message(b"second-pubkey", bytes_of(&self.second_pubkey));
        transcript.append_message(b"first-ciphertext", bytes_of(&self.first_ciphertext));
        transcript.append_message(b"second-ciphertext", bytes_of(&self.second_ciphertext));

        transcript
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ciphertext_ciphertext_instruction_correctness() {
        let first_keypair = ElGamalKeypair::new_rand();
        let second_keypair = ElGamalKeypair::new_rand();

        let amount: u64 = 0;
        let first_ciphertext = first_keypair.pubkey().encrypt(amount);

        let second_opening = PedersenOpening::new_rand();
        let second_ciphertext = second_keypair
            .pubkey()
            .encrypt_with(amount, &second_opening);

        let proof_data = CiphertextCiphertextEqualityProofData::new(
            &first_keypair,
            second_keypair.pubkey(),
            &first_ciphertext,
            &second_ciphertext,
            &second_opening,
            amount,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());

        let amount: u64 = 55;
        let first_ciphertext = first_keypair.pubkey().encrypt(amount);

        let second_opening = PedersenOpening::new_rand();
        let second_ciphertext = second_keypair
            .pubkey()
            .encrypt_with(amount, &second_opening);

        let proof_data = CiphertextCiphertextEqualityProofData::new(
            &first_keypair,
            second_keypair.pubkey(),
            &first_ciphertext,
            &second_ciphertext,
            &second_opening,
            amount,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());

        let amount = u64::MAX;
        let first_ciphertext = first_keypair.pubkey().encrypt(amount);

        let second_opening = PedersenOpening::new_rand();
        let second_ciphertext = second_keypair
            .pubkey()
            .encrypt_with(amount, &second_opening);

        let proof_data = CiphertextCiphertextEqualityProofData::new(
            &first_keypair,
            second_keypair.pubkey(),
            &first_ciphertext,
            &second_ciphertext,
            &second_opening,
            amount,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());
    }
}
