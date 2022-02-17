use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::PedersenOpening,
        },
        errors::ProofError,
        instruction::Verifiable,
        sigma_proofs::equality_proof::CtxtCtxtEqualityProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
    std::convert::TryInto,
};

/// This struct includes the cryptographic proof *and* the account data information needed to verify
/// the proof
///
/// - The pre-instruction should call WithdrawData::verify_proof(&self)
/// - The actual program should check that `current_ct` is consistent with what is
///   currently stored in the confidential token account TODO: update this statement
///
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct WithdrawWithheldTokensData {
    pub pubkey_withdraw_withheld_authority: pod::ElGamalPubkey,

    pub pubkey_dest: pod::ElGamalPubkey,

    pub ciphertext_withdraw_withheld_authority: pod::ElGamalCiphertext,

    pub ciphertext_dest: pod::ElGamalCiphertext,

    pub proof: WithdrawWithheldTokensProof,
}

impl WithdrawWithheldTokensData {
    #[cfg(not(target_arch = "bpf"))]
    pub fn new(
        keypair_withdraw_withheld_authority: &ElGamalKeypair,
        pubkey_dest: &ElGamalPubkey,
        ciphertext_withdraw_withheld_authority: &ElGamalCiphertext,
        amount: u64,
    ) -> Result<Self, ProofError> {
        let opening_dest = PedersenOpening::new_rand();
        let ciphertext_dest = pubkey_dest.encrypt_with(amount, &opening_dest);

        let pod_pubkey_withdraw_withheld_authority =
            pod::ElGamalPubkey(keypair_withdraw_withheld_authority.public.to_bytes());
        let pod_pubkey_dest = pod::ElGamalPubkey(pubkey_dest.to_bytes());
        let pod_ciphertext_withdraw_withheld_authority =
            pod::ElGamalCiphertext(ciphertext_withdraw_withheld_authority.to_bytes());
        let pod_ciphertext_dest = pod::ElGamalCiphertext(ciphertext_dest.to_bytes());

        let mut transcript = WithdrawWithheldTokensProof::transcript_new(
            &pod_pubkey_withdraw_withheld_authority,
            &pod_pubkey_dest,
            &pod_ciphertext_withdraw_withheld_authority,
            &pod_ciphertext_dest,
        );

        let proof = WithdrawWithheldTokensProof::new(
            keypair_withdraw_withheld_authority,
            pubkey_dest,
            ciphertext_withdraw_withheld_authority,
            amount,
            &opening_dest,
            &mut transcript,
        );

        Ok(Self {
            pubkey_withdraw_withheld_authority: pod_pubkey_withdraw_withheld_authority,
            pubkey_dest: pod_pubkey_dest,
            ciphertext_withdraw_withheld_authority: pod_ciphertext_withdraw_withheld_authority,
            ciphertext_dest: pod_ciphertext_dest,
            proof,
        })
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for WithdrawWithheldTokensData {
    fn verify(&self) -> Result<(), ProofError> {
        let mut transcript = WithdrawWithheldTokensProof::transcript_new(
            &self.pubkey_withdraw_withheld_authority,
            &self.pubkey_dest,
            &self.ciphertext_withdraw_withheld_authority,
            &self.ciphertext_dest,
        );

        let pubkey_withdraw_withheld_authority =
            self.pubkey_withdraw_withheld_authority.try_into()?;
        let pubkey_dest = self.pubkey_dest.try_into()?;
        let ciphertext_withdraw_withheld_authority =
            self.ciphertext_withdraw_withheld_authority.try_into()?;
        let ciphertext_dest = self.ciphertext_dest.try_into()?;

        self.proof.verify(
            &pubkey_withdraw_withheld_authority,
            &pubkey_dest,
            &ciphertext_withdraw_withheld_authority,
            &ciphertext_dest,
            &mut transcript,
        )
    }
}

/// This struct represents the cryptographic proof component that certifies the account's solvency
/// for withdrawal
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct WithdrawWithheldTokensProof {
    pub proof: pod::CtxtCtxtEqualityProof,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl WithdrawWithheldTokensProof {
    fn transcript_new(
        pubkey_withdraw_withheld_authority: &pod::ElGamalPubkey,
        pubkey_dest: &pod::ElGamalPubkey,
        ciphertext_withdraw_withheld_authority: &pod::ElGamalCiphertext,
        ciphertext_dest: &pod::ElGamalCiphertext,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"WithdrawWithheldTokensProof");

        transcript.append_pubkey(
            b"withdraw-withheld-authority-pubkey",
            pubkey_withdraw_withheld_authority,
        );
        transcript.append_pubkey(b"dest-pubkey", pubkey_dest);

        transcript.append_ciphertext(
            b"ciphertext-withdraw-withheld-authority",
            ciphertext_withdraw_withheld_authority,
        );
        transcript.append_ciphertext(b"ciphertext-dest", ciphertext_dest);

        transcript
    }

    pub fn new(
        keypair_withdraw_withheld_authority: &ElGamalKeypair,
        pubkey_dest: &ElGamalPubkey,
        ciphertext_withdraw_withheld_authority: &ElGamalCiphertext,
        amount: u64,
        opening_dest: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        let equality_proof = CtxtCtxtEqualityProof::new(
            keypair_withdraw_withheld_authority,
            pubkey_dest,
            ciphertext_withdraw_withheld_authority,
            amount,
            opening_dest,
            transcript,
        );

        Self {
            proof: equality_proof.into(),
        }
    }

    pub fn verify(
        &self,
        pubkey_source: &ElGamalPubkey,
        pubkey_dest: &ElGamalPubkey,
        ciphertext_source: &ElGamalCiphertext,
        ciphertext_dest: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        let proof: CtxtCtxtEqualityProof = self.proof.try_into()?;
        proof.verify(
            pubkey_source,
            pubkey_dest,
            ciphertext_source,
            ciphertext_dest,
            transcript,
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_close_account_correctness() {
        let keypair_withdraw_withheld_authority = ElGamalKeypair::new_rand();
        let keypair_dest = ElGamalKeypair::new_rand();

        let amount: u64 = 55;
        let ciphertext_withdraw_withheld_authority =
            keypair_withdraw_withheld_authority.public.encrypt(amount);

        let withdraw_withheld_tokens_data = WithdrawWithheldTokensData::new(
            &keypair_withdraw_withheld_authority,
            &keypair_dest.public,
            &ciphertext_withdraw_withheld_authority,
            amount,
        )
        .unwrap();

        assert!(withdraw_withheld_tokens_data.verify().is_ok());
    }
}
