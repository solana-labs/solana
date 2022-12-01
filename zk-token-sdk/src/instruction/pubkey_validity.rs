use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::elgamal::{ElGamalKeypair, ElGamalPubkey},
        errors::ProofError,
        instruction::Verifiable,
        sigma_proofs::pubkey_proof::PubkeySigmaProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
    std::convert::TryInto,
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
    /// The public key to be proved
    pub pubkey: pod::ElGamalPubkey,

    /// Proof that the public key is well-formed
    pub proof: PubkeyValidityProof, // 64 bytes
}

#[cfg(not(target_os = "solana"))]
impl PubkeyValidityData {
    pub fn new(keypair: &ElGamalKeypair) -> Result<Self, ProofError> {
        let pod_pubkey = pod::ElGamalPubkey(keypair.public.to_bytes());

        let mut transcript = PubkeyValidityProof::transcript_new(&pod_pubkey);

        let proof = PubkeyValidityProof::new(keypair, &mut transcript);

        Ok(PubkeyValidityData {
            pubkey: pod_pubkey,
            proof,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl Verifiable for PubkeyValidityData {
    fn verify(&self) -> Result<(), ProofError> {
        let mut transcript = PubkeyValidityProof::transcript_new(&self.pubkey);
        let pubkey = self.pubkey.try_into()?;
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
        assert!(pubkey_validity_data.verify().is_ok());
    }
}
