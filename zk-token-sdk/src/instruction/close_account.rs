use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
        errors::ProofError,
        instruction::Verifiable,
        sigma_proofs::zero_balance_proof::ZeroBalanceProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
    std::convert::TryInto,
};

/// This struct includes the cryptographic proof *and* the account data information needed to verify
/// the proof
///
/// - The pre-instruction should call CloseAccountData::verify_proof(&self)
/// - The actual program should check that `balance` is consistent with what is
///   currently stored in the confidential token account
///
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct CloseAccountData {
    /// The source account ElGamal pubkey
    pub pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The source account available balance in encrypted form
    pub ciphertext: pod::ElGamalCiphertext, // 64 bytes

    /// Proof that the source account available balance is zero
    pub proof: CloseAccountProof, // 64 bytes
}

#[cfg(not(target_os = "solana"))]
impl CloseAccountData {
    pub fn new(
        keypair: &ElGamalKeypair,
        ciphertext: &ElGamalCiphertext,
    ) -> Result<Self, ProofError> {
        let pod_pubkey = pod::ElGamalPubkey(keypair.public.to_bytes());
        let pod_ciphertext = pod::ElGamalCiphertext(ciphertext.to_bytes());

        let mut transcript = CloseAccountProof::transcript_new(&pod_pubkey, &pod_ciphertext);

        let proof = CloseAccountProof::new(keypair, ciphertext, &mut transcript);

        Ok(CloseAccountData {
            pubkey: pod_pubkey,
            ciphertext: pod_ciphertext,
            proof,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl Verifiable for CloseAccountData {
    fn verify(&self) -> Result<(), ProofError> {
        let mut transcript = CloseAccountProof::transcript_new(&self.pubkey, &self.ciphertext);

        let pubkey = self.pubkey.try_into()?;
        let ciphertext = self.ciphertext.try_into()?;
        self.proof.verify(&pubkey, &ciphertext, &mut transcript)
    }
}

/// This struct represents the cryptographic proof component that certifies that the encrypted
/// balance is zero
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct CloseAccountProof {
    pub proof: pod::ZeroBalanceProof,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl CloseAccountProof {
    fn transcript_new(
        pubkey: &pod::ElGamalPubkey,
        ciphertext: &pod::ElGamalCiphertext,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"CloseAccountProof");

        transcript.append_pubkey(b"pubkey", pubkey);
        transcript.append_ciphertext(b"ciphertext", ciphertext);

        transcript
    }

    pub fn new(
        keypair: &ElGamalKeypair,
        ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Self {
        let proof = ZeroBalanceProof::new(keypair, ciphertext, transcript);

        Self {
            proof: proof.into(),
        }
    }

    pub fn verify(
        &self,
        pubkey: &ElGamalPubkey,
        ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        let proof: ZeroBalanceProof = self.proof.try_into()?;
        proof.verify(pubkey, ciphertext, transcript)?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_close_account_correctness() {
        let keypair = ElGamalKeypair::new_rand();

        // general case: encryption of 0
        let ciphertext = keypair.public.encrypt(0_u64);
        let close_account_data = CloseAccountData::new(&keypair, &ciphertext).unwrap();
        assert!(close_account_data.verify().is_ok());

        // general case: encryption of > 0
        let ciphertext = keypair.public.encrypt(1_u64);
        let close_account_data = CloseAccountData::new(&keypair, &ciphertext).unwrap();
        assert!(close_account_data.verify().is_err());
    }
}
