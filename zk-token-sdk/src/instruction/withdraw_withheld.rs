use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_os = "solana"))]
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
/// - The pre-instruction should call WithdrawWithheldTokensData::verify_proof(&self)
/// - The actual program should check that the ciphertext in this struct is consistent with what is
/// currently stored in the confidential token account
///
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct WithdrawWithheldTokensData {
    pub withdraw_withheld_authority_pubkey: pod::ElGamalPubkey,

    pub destination_pubkey: pod::ElGamalPubkey,

    pub withdraw_withheld_authority_ciphertext: pod::ElGamalCiphertext,

    pub destination_ciphertext: pod::ElGamalCiphertext,

    pub proof: WithdrawWithheldTokensProof,
}

impl WithdrawWithheldTokensData {
    #[cfg(not(target_os = "solana"))]
    pub fn new(
        withdraw_withheld_authority_keypair: &ElGamalKeypair,
        destination_pubkey: &ElGamalPubkey,
        withdraw_withheld_authority_ciphertext: &ElGamalCiphertext,
        amount: u64,
    ) -> Result<Self, ProofError> {
        // encrypt withdraw amount under destination public key
        let destination_opening = PedersenOpening::new_rand();
        let destination_ciphertext = destination_pubkey.encrypt_with(amount, &destination_opening);

        let pod_withdraw_withheld_authority_pubkey =
            pod::ElGamalPubkey(withdraw_withheld_authority_keypair.public.to_bytes());
        let pod_destination_pubkey = pod::ElGamalPubkey(destination_pubkey.to_bytes());
        let pod_withdraw_withheld_authority_ciphertext =
            pod::ElGamalCiphertext(withdraw_withheld_authority_ciphertext.to_bytes());
        let pod_destination_ciphertext = pod::ElGamalCiphertext(destination_ciphertext.to_bytes());

        let mut transcript = WithdrawWithheldTokensProof::transcript_new(
            &pod_withdraw_withheld_authority_pubkey,
            &pod_destination_pubkey,
            &pod_withdraw_withheld_authority_ciphertext,
            &pod_destination_ciphertext,
        );

        let proof = WithdrawWithheldTokensProof::new(
            withdraw_withheld_authority_keypair,
            destination_pubkey,
            withdraw_withheld_authority_ciphertext,
            amount,
            &destination_opening,
            &mut transcript,
        );

        Ok(Self {
            withdraw_withheld_authority_pubkey: pod_withdraw_withheld_authority_pubkey,
            destination_pubkey: pod_destination_pubkey,
            withdraw_withheld_authority_ciphertext: pod_withdraw_withheld_authority_ciphertext,
            destination_ciphertext: pod_destination_ciphertext,
            proof,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl Verifiable for WithdrawWithheldTokensData {
    fn verify(&self) -> Result<(), ProofError> {
        let mut transcript = WithdrawWithheldTokensProof::transcript_new(
            &self.withdraw_withheld_authority_pubkey,
            &self.destination_pubkey,
            &self.withdraw_withheld_authority_ciphertext,
            &self.destination_ciphertext,
        );

        let withdraw_withheld_authority_pubkey =
            self.withdraw_withheld_authority_pubkey.try_into()?;
        let destination_pubkey = self.destination_pubkey.try_into()?;
        let withdraw_withheld_authority_ciphertext =
            self.withdraw_withheld_authority_ciphertext.try_into()?;
        let destination_ciphertext = self.destination_ciphertext.try_into()?;

        self.proof.verify(
            &withdraw_withheld_authority_pubkey,
            &destination_pubkey,
            &withdraw_withheld_authority_ciphertext,
            &destination_ciphertext,
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
#[cfg(not(target_os = "solana"))]
impl WithdrawWithheldTokensProof {
    fn transcript_new(
        withdraw_withheld_authority_pubkey: &pod::ElGamalPubkey,
        destination_pubkey: &pod::ElGamalPubkey,
        withdraw_withheld_authority_ciphertext: &pod::ElGamalCiphertext,
        destination_ciphertext: &pod::ElGamalCiphertext,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"WithdrawWithheldTokensProof");

        transcript.append_pubkey(
            b"withdraw-withheld-authority-pubkey",
            withdraw_withheld_authority_pubkey,
        );
        transcript.append_pubkey(b"dest-pubkey", destination_pubkey);

        transcript.append_ciphertext(
            b"ciphertext-withdraw-withheld-authority",
            withdraw_withheld_authority_ciphertext,
        );
        transcript.append_ciphertext(b"ciphertext-dest", destination_ciphertext);

        transcript
    }

    pub fn new(
        withdraw_withheld_authority_keypair: &ElGamalKeypair,
        destination_pubkey: &ElGamalPubkey,
        withdraw_withheld_authority_ciphertext: &ElGamalCiphertext,
        amount: u64,
        destination_opening: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        let equality_proof = CtxtCtxtEqualityProof::new(
            withdraw_withheld_authority_keypair,
            destination_pubkey,
            withdraw_withheld_authority_ciphertext,
            amount,
            destination_opening,
            transcript,
        );

        Self {
            proof: equality_proof.into(),
        }
    }

    pub fn verify(
        &self,
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        source_ciphertext: &ElGamalCiphertext,
        destination_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        let proof: CtxtCtxtEqualityProof = self.proof.try_into()?;
        proof.verify(
            source_pubkey,
            destination_pubkey,
            source_ciphertext,
            destination_ciphertext,
            transcript,
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_withdraw_withheld() {
        let withdraw_withheld_authority_keypair = ElGamalKeypair::new_rand();
        let dest_keypair = ElGamalKeypair::new_rand();

        let amount: u64 = 0;
        let withdraw_withheld_authority_ciphertext =
            withdraw_withheld_authority_keypair.public.encrypt(amount);

        let withdraw_withheld_tokens_data = WithdrawWithheldTokensData::new(
            &withdraw_withheld_authority_keypair,
            &dest_keypair.public,
            &withdraw_withheld_authority_ciphertext,
            amount,
        )
        .unwrap();

        assert!(withdraw_withheld_tokens_data.verify().is_ok());

        let amount: u64 = 55;
        let withdraw_withheld_authority_ciphertext =
            withdraw_withheld_authority_keypair.public.encrypt(amount);

        let withdraw_withheld_tokens_data = WithdrawWithheldTokensData::new(
            &withdraw_withheld_authority_keypair,
            &dest_keypair.public,
            &withdraw_withheld_authority_ciphertext,
            amount,
        )
        .unwrap();

        assert!(withdraw_withheld_tokens_data.verify().is_ok());

        let amount = u64::max_value();
        let withdraw_withheld_authority_ciphertext =
            withdraw_withheld_authority_keypair.public.encrypt(amount);

        let withdraw_withheld_tokens_data = WithdrawWithheldTokensData::new(
            &withdraw_withheld_authority_keypair,
            &dest_keypair.public,
            &withdraw_withheld_authority_ciphertext,
            amount,
        )
        .unwrap();

        assert!(withdraw_withheld_tokens_data.verify().is_ok());
    }
}
