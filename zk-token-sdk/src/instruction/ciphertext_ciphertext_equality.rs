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
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::PedersenOpening,
        },
        errors::ProofError,
        sigma_proofs::ciphertext_ciphertext_equality_proof::CiphertextCiphertextEqualityProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
    std::convert::TryInto,
};
use {
    crate::{
        instruction::{ProofType, ZkProofData},
        zk_token_elgamal::pod,
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

    pub proof: pod::CiphertextCiphertextEqualityProof,
}

/// The context data needed to verify a ciphertext-ciphertext equality proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct CiphertextCiphertextEqualityProofContext {
    pub source_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes

    pub source_ciphertext: pod::ElGamalCiphertext, // 64 bytes

    pub destination_ciphertext: pod::ElGamalCiphertext, // 64 bytes
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
    ) -> Result<Self, ProofError> {
        let pod_source_pubkey = pod::ElGamalPubkey(source_keypair.pubkey().to_bytes());
        let pod_destination_pubkey = pod::ElGamalPubkey(destination_pubkey.to_bytes());
        let pod_source_ciphertext = pod::ElGamalCiphertext(source_ciphertext.to_bytes());
        let pod_destination_ciphertext = pod::ElGamalCiphertext(destination_ciphertext.to_bytes());

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
    fn verify_proof(&self) -> Result<(), ProofError> {
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
        let mut transcript = Transcript::new(b"CiphertextCiphertextEqualityProof");

        transcript.append_pubkey(b"source-pubkey", &self.source_pubkey);
        transcript.append_pubkey(b"destination-pubkey", &self.destination_pubkey);

        transcript.append_ciphertext(b"source-ciphertext", &self.source_ciphertext);
        transcript.append_ciphertext(b"destination-ciphertext", &self.destination_ciphertext);

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

        let amount = u64::max_value();
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
