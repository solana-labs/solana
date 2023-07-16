//! The public-key validity proof instruction.
//!
//! A public-key validity proof system is defined with respect to an ElGamal public key. The proof
//! certifies that a given public key is a valid ElGamal public key (i.e. the prover knows a
//! corresponding secret key). To generate the proof, a prover must provide the secret key for the
//! public key.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::elgamal::ElGamalKeypair, errors::ProofError,
        sigma_proofs::pubkey_proof::PubkeyValidityProof, transcript::TranscriptProtocol,
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

/// The instruction data that is needed for the `ProofInstruction::VerifyPubkeyValidity`
/// instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct PubkeyValidityData {
    /// The context data for the public key validity proof
    pub context: PubkeyValidityProofContext, // 32 bytes

    /// Proof that the public key is well-formed
    pub proof: pod::PubkeyValidityProof, // 64 bytes
}

/// The context data needed to verify a pubkey validity proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct PubkeyValidityProofContext {
    /// The public key to be proved
    pub pubkey: pod::ElGamalPubkey, // 32 bytes
}

#[cfg(not(target_os = "solana"))]
impl PubkeyValidityData {
    pub fn new(keypair: &ElGamalKeypair) -> Result<Self, ProofError> {
        let pod_pubkey = pod::ElGamalPubkey(keypair.pubkey().to_bytes());

        let context = PubkeyValidityProofContext { pubkey: pod_pubkey };

        let mut transcript = context.new_transcript();
        let proof = PubkeyValidityProof::new(keypair, &mut transcript).into();

        Ok(PubkeyValidityData { context, proof })
    }
}

impl ZkProofData<PubkeyValidityProofContext> for PubkeyValidityData {
    const PROOF_TYPE: ProofType = ProofType::PubkeyValidity;

    fn context_data(&self) -> &PubkeyValidityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context.new_transcript();
        let pubkey = self.context.pubkey.try_into()?;
        let proof: PubkeyValidityProof = self.proof.try_into()?;
        proof.verify(&pubkey, &mut transcript).map_err(|e| e.into())
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl PubkeyValidityProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"PubkeyProof");
        transcript.append_pubkey(b"pubkey", &self.pubkey);
        transcript
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_pubkey_validity_instruction_correctness() {
        let keypair = ElGamalKeypair::new_rand();

        let pubkey_validity_data = PubkeyValidityData::new(&keypair).unwrap();
        assert!(pubkey_validity_data.verify_proof().is_ok());
    }
}
