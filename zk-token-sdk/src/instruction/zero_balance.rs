//! The zero-balance proof instruction.
//!
//! A zero-balance proof is defined with respect to a twisted ElGamal ciphertext. The proof
//! certifies that a given ciphertext encrypts the message 0 in the field (`Scalar::zero()`). To
//! generate the proof, a prover must provide the decryption key for the ciphertext.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::elgamal::{ElGamalCiphertext, ElGamalKeypair},
        errors::ProofError,
        sigma_proofs::zero_balance_proof::ZeroBalanceProof,
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

/// The instruction data that is needed for the `ProofInstruction::ZeroBalance` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ZeroBalanceProofData {
    /// The context data for the zero-balance proof
    pub context: ZeroBalanceProofContext, // 96 bytes

    /// Proof that the source account available balance is zero
    pub proof: pod::ZeroBalanceProof, // 96 bytes
}

/// The context data needed to verify a zero-balance proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ZeroBalanceProofContext {
    /// The source account ElGamal pubkey
    pub pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The source account available balance in encrypted form
    pub ciphertext: pod::ElGamalCiphertext, // 64 bytes
}

#[cfg(not(target_os = "solana"))]
impl ZeroBalanceProofData {
    pub fn new(
        keypair: &ElGamalKeypair,
        ciphertext: &ElGamalCiphertext,
    ) -> Result<Self, ProofError> {
        let pod_pubkey = pod::ElGamalPubkey(keypair.pubkey().to_bytes());
        let pod_ciphertext = pod::ElGamalCiphertext(ciphertext.to_bytes());

        let context = ZeroBalanceProofContext {
            pubkey: pod_pubkey,
            ciphertext: pod_ciphertext,
        };

        let mut transcript = context.new_transcript();
        let proof = ZeroBalanceProof::new(keypair, ciphertext, &mut transcript).into();

        Ok(ZeroBalanceProofData { context, proof })
    }
}

impl ZkProofData<ZeroBalanceProofContext> for ZeroBalanceProofData {
    const PROOF_TYPE: ProofType = ProofType::ZeroBalance;

    fn context_data(&self) -> &ZeroBalanceProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context.new_transcript();
        let pubkey = self.context.pubkey.try_into()?;
        let ciphertext = self.context.ciphertext.try_into()?;
        let proof: ZeroBalanceProof = self.proof.try_into()?;
        proof
            .verify(&pubkey, &ciphertext, &mut transcript)
            .map_err(|e| e.into())
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl ZeroBalanceProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"ZeroBalanceProof");

        transcript.append_pubkey(b"pubkey", &self.pubkey);
        transcript.append_ciphertext(b"ciphertext", &self.ciphertext);

        transcript
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_zero_balance_proof_instruction_correctness() {
        let keypair = ElGamalKeypair::new_rand();

        // general case: encryption of 0
        let ciphertext = keypair.pubkey().encrypt(0_u64);
        let zero_balance_proof_data = ZeroBalanceProofData::new(&keypair, &ciphertext).unwrap();
        assert!(zero_balance_proof_data.verify_proof().is_ok());

        // general case: encryption of > 0
        let ciphertext = keypair.pubkey().encrypt(1_u64);
        let zero_balance_proof_data = ZeroBalanceProofData::new(&keypair, &ciphertext).unwrap();
        assert!(zero_balance_proof_data.verify_proof().is_err());
    }
}
