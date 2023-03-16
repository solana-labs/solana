#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::elgamal::{ElGamalKeypair, ElGamalPubkey},
        errors::ProofError,
        sigma_proofs::pubkey_proof::PubkeySigmaProof,
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

/// This struct includes the cryptographic proof *and* the account data information needed to
/// verify the proof
///
/// - The pre-instruction should call PubkeyValidityData::verify_proof(&self)
/// - The actual program should check that the public key in this struct is consistent with what is
/// stored in the confidential token account
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct PubkeyValidityData {
    /// The context data for the public key validity proof
    pub context: PubkeyValidityProofContext,

    /// Proof that the public key is well-formed
    pub proof: PubkeyValidityProof, // 64 bytes
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct PubkeyValidityProofContext {
    /// The public key to be proved
    pub pubkey: pod::ElGamalPubkey, // 32 bytes
}

#[cfg(not(target_os = "solana"))]
impl PubkeyValidityData {
    pub fn new(keypair: &ElGamalKeypair) -> Result<Self, ProofError> {
        let pod_pubkey = pod::ElGamalPubkey(keypair.public.to_bytes());

        let context = PubkeyValidityProofContext { pubkey: pod_pubkey };

        let mut transcript = PubkeyValidityProof::transcript_new(&pod_pubkey);
        let proof = PubkeyValidityProof::new(keypair, &mut transcript);

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
        let mut transcript = PubkeyValidityProof::transcript_new(&self.context.pubkey);
        let pubkey = self.context.pubkey.try_into()?;
        self.proof.verify(&pubkey, &mut transcript)
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct PubkeyValidityProof {
    /// Associated public-key sigma proof
    pub proof: pod::PubkeySigmaProof,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl PubkeyValidityProof {
    fn transcript_new(pubkey: &pod::ElGamalPubkey) -> Transcript {
        let mut transcript = Transcript::new(b"PubkeyProof");
        transcript.append_pubkey(b"pubkey", pubkey);
        transcript
    }

    pub fn new(keypair: &ElGamalKeypair, transcript: &mut Transcript) -> Self {
        let proof = PubkeySigmaProof::new(keypair, transcript);
        Self {
            proof: proof.into(),
        }
    }

    pub fn verify(
        &self,
        pubkey: &ElGamalPubkey,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        let proof: PubkeySigmaProof = self.proof.try_into()?;
        proof.verify(pubkey, transcript)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_pubkey_validity_correctness() {
        let keypair = ElGamalKeypair::new_rand();

        let pubkey_validity_data = PubkeyValidityData::new(&keypair).unwrap();
        assert!(pubkey_validity_data.verify_proof().is_ok());
    }
}
