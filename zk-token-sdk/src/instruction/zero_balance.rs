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

/// This struct includes the cryptographic proof *and* the account data information needed to verify
/// the proof
///
/// - The pre-instruction should call ZeroBalanceProofData::verify_proof(&self)
/// - The actual program should check that `balance` is consistent with what is
///   currently stored in the confidential token account
///
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ZeroBalanceProofData {
    /// The context data for the zero-balance proof
    pub context: ZeroBalanceProofContext,

    /// Proof that the source account available balance is zero
    pub proof: pod::ZeroBalanceProof, // 96 bytes
}

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
        let pod_pubkey = pod::ElGamalPubkey(keypair.public.to_bytes());
        let pod_ciphertext = pod::ElGamalCiphertext(ciphertext.to_bytes());

        let context = ZeroBalanceProofContext {
            pubkey: pod_pubkey,
            ciphertext: pod_ciphertext,
        };

        let mut transcript = ZeroBalanceProof::transcript_new(&pod_pubkey, &pod_ciphertext);
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
        let mut transcript =
            ZeroBalanceProof::transcript_new(&self.context.pubkey, &self.context.ciphertext);

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
impl ZeroBalanceProof {
    fn transcript_new(
        pubkey: &pod::ElGamalPubkey,
        ciphertext: &pod::ElGamalCiphertext,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"ZeroBalanceProof");

        transcript.append_pubkey(b"pubkey", pubkey);
        transcript.append_ciphertext(b"ciphertext", ciphertext);

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
        let ciphertext = keypair.public.encrypt(0_u64);
        let zero_balance_proof_data = ZeroBalanceProofData::new(&keypair, &ciphertext).unwrap();
        assert!(zero_balance_proof_data.verify_proof().is_ok());

        // general case: encryption of > 0
        let ciphertext = keypair.public.encrypt(1_u64);
        let zero_balance_proof_data = ZeroBalanceProofData::new(&keypair, &ciphertext).unwrap();
        assert!(zero_balance_proof_data.verify_proof().is_err());
    }
}
