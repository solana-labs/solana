#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair},
            pedersen::{PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        sigma_proofs::equality_proof::CtxtCommEqualityProof,
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

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct CtxtCommEqualityProofData {
    /// The context data for the ciphertext-commitment equality proof instruction
    pub context: CtxtCommEqualityProofContext, // 128 bytes

    /// Proof that an ElGamal ciphertext is valid
    pub proof: pod::CtxtCommEqualityProof, // 192 bytes
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct CtxtCommEqualityProofContext {
    /// The ElGamal pubkey
    pub pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The ciphertext encrypted under the ElGamal pubkey
    pub ciphertext: pod::ElGamalCiphertext, // 64 bytes

    /// The Pedersen commitment
    pub commitment: pod::PedersenCommitment, // 32 bytes
}

#[cfg(not(target_os = "solana"))]
impl CtxtCommEqualityProofData {
    pub fn new(
        keypair: &ElGamalKeypair,
        ciphertext: &ElGamalCiphertext,
        commitment: &PedersenCommitment,
        amount: u64,
        opening: &PedersenOpening,
    ) -> Result<Self, ProofError> {
        let context = CtxtCommEqualityProofContext {
            pubkey: pod::ElGamalPubkey(keypair.public.to_bytes()),
            ciphertext: pod::ElGamalCiphertext(ciphertext.to_bytes()),
            commitment: pod::PedersenCommitment(commitment.to_bytes()),
        };

        let mut transcript = CtxtCommEqualityProofData::transcript_new(&context);

        let proof =
            CtxtCommEqualityProof::new(keypair, ciphertext, amount, opening, &mut transcript);

        Ok(CtxtCommEqualityProofData {
            context,
            proof: proof.into(),
        })
    }
}

impl ZkProofData<CtxtCommEqualityProofContext> for CtxtCommEqualityProofData {
    const PROOF_TYPE: ProofType = ProofType::CtxtCommEqualityProof;

    fn context_data(&self) -> &CtxtCommEqualityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = CtxtCommEqualityProofData::transcript_new(&self.context);

        let pubkey = self.context.pubkey.try_into()?;
        let ciphertext = self.context.ciphertext.try_into()?;
        let commitment = self.context.commitment.try_into()?;

        let proof: CtxtCommEqualityProof = self.proof.try_into()?;
        proof
            .verify(&pubkey, &ciphertext, &commitment, &mut transcript)
            .map_err(|e| e.into())
    }
}

impl CtxtCommEqualityProofData {
    fn transcript_new(context: &CtxtCommEqualityProofContext) -> Transcript {
        let mut transcript = Transcript::new(b"CtxtCommEqualityProof");

        transcript.append_pubkey(b"pubkey", &context.pubkey);
        transcript.append_ciphertext(b"ciphertext", &context.ciphertext);
        transcript.append_commitment(b"commitment", &context.commitment);

        transcript
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{elgamal::ElGamalKeypair, pedersen::Pedersen},
    };

    #[test]
    fn test_ctxt_comm_equality_proof_correctness() {
        let keypair = ElGamalKeypair::new_rand();
        let amount: u64 = 55;

        let ciphertext = keypair.public.encrypt(amount);
        let (commitment, opening) = Pedersen::new(amount);

        let ctxt_comm_equality_proof_data =
            CtxtCommEqualityProofData::new(&keypair, &ciphertext, &commitment, amount, &opening)
                .unwrap();

        assert!(ctxt_comm_equality_proof_data.verify_proof().is_ok());
    }
}
