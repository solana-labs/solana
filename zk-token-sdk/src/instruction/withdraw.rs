use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamal, ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::{Pedersen, PedersenCommitment},
        },
        errors::ProofError,
        instruction::Verifiable,
        range_proof::RangeProof,
        sigma_proofs::equality_proof::CtxtCommEqualityProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
    std::convert::TryInto,
};

#[cfg(not(target_os = "solana"))]
const WITHDRAW_AMOUNT_BIT_LENGTH: usize = 64;

/// This struct includes the cryptographic proof *and* the account data information needed to verify
/// the proof
///
/// - The pre-instruction should call WithdrawData::verify_proof(&self)
/// - The actual program should check that `current_ct` is consistent with what is
///   currently stored in the confidential token account TODO: update this statement
///
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct WithdrawData {
    /// The source account ElGamal pubkey
    pub pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The source account available balance *after* the withdraw (encrypted by
    /// `source_pk`
    pub final_ciphertext: pod::ElGamalCiphertext, // 64 bytes

    /// Range proof
    pub proof: WithdrawProof, // 736 bytes
}

impl WithdrawData {
    #[cfg(not(target_os = "solana"))]
    pub fn new(
        amount: u64,
        keypair: &ElGamalKeypair,
        current_balance: u64,
        current_ciphertext: &ElGamalCiphertext,
    ) -> Result<Self, ProofError> {
        // subtract withdraw amount from current balance
        //
        // errors if current_balance < amount
        let final_balance = current_balance
            .checked_sub(amount)
            .ok_or(ProofError::Generation)?;

        // encode withdraw amount as an ElGamal ciphertext and subtract it from
        // current source balance
        let final_ciphertext = current_ciphertext - &ElGamal::encode(amount);

        let pod_pubkey = pod::ElGamalPubkey((&keypair.public).to_bytes());
        let pod_final_ciphertext: pod::ElGamalCiphertext = final_ciphertext.into();
        let mut transcript = WithdrawProof::transcript_new(&pod_pubkey, &pod_final_ciphertext);
        let proof = WithdrawProof::new(keypair, final_balance, &final_ciphertext, &mut transcript);

        Ok(Self {
            pubkey: pod_pubkey,
            final_ciphertext: pod_final_ciphertext,
            proof,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl Verifiable for WithdrawData {
    fn verify(&self) -> Result<(), ProofError> {
        let mut transcript = WithdrawProof::transcript_new(&self.pubkey, &self.final_ciphertext);

        let elgamal_pubkey = self.pubkey.try_into()?;
        let final_balance_ciphertext = self.final_ciphertext.try_into()?;
        self.proof
            .verify(&elgamal_pubkey, &final_balance_ciphertext, &mut transcript)
    }
}

/// This struct represents the cryptographic proof component that certifies the account's solvency
/// for withdrawal
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct WithdrawProof {
    /// New Pedersen commitment
    pub commitment: pod::PedersenCommitment,

    /// Associated equality proof
    pub equality_proof: pod::CtxtCommEqualityProof,

    /// Associated range proof
    pub range_proof: pod::RangeProof64, // 672 bytes
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl WithdrawProof {
    fn transcript_new(
        pubkey: &pod::ElGamalPubkey,
        ciphertext: &pod::ElGamalCiphertext,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"WithdrawProof");

        transcript.append_pubkey(b"pubkey", pubkey);
        transcript.append_ciphertext(b"ciphertext", ciphertext);

        transcript
    }

    pub fn new(
        keypair: &ElGamalKeypair,
        final_balance: u64,
        final_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Self {
        // generate a Pedersen commitment for `final_balance`
        let (commitment, opening) = Pedersen::new(final_balance);
        let pod_commitment: pod::PedersenCommitment = commitment.into();

        transcript.append_commitment(b"commitment", &pod_commitment);

        // generate equality_proof
        let equality_proof = CtxtCommEqualityProof::new(
            keypair,
            final_ciphertext,
            final_balance,
            &opening,
            transcript,
        );

        let range_proof =
            RangeProof::new(vec![final_balance], vec![64], vec![&opening], transcript);

        Self {
            commitment: pod_commitment,
            equality_proof: equality_proof.try_into().expect("equality proof"),
            range_proof: range_proof.try_into().expect("range proof"),
        }
    }

    pub fn verify(
        &self,
        pubkey: &ElGamalPubkey,
        final_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        transcript.append_commitment(b"commitment", &self.commitment);

        let commitment: PedersenCommitment = self.commitment.try_into()?;
        let equality_proof: CtxtCommEqualityProof = self.equality_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // verify equality proof
        //
        // TODO: we can also consider verifying equality and range proof in a batch
        equality_proof.verify(pubkey, final_ciphertext, &commitment, transcript)?;

        // verify range proof
        //
        // TODO: double compressing here - consider modifying range proof input type to `PedersenCommitment`
        range_proof.verify(
            vec![&commitment],
            vec![WITHDRAW_AMOUNT_BIT_LENGTH],
            transcript,
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::elgamal::ElGamalKeypair};

    #[test]
    fn test_withdraw_correctness() {
        // generate and verify proof for the proper setting
        let keypair = ElGamalKeypair::new_rand();

        let current_balance: u64 = 77;
        let current_ciphertext = keypair.public.encrypt(current_balance);

        let withdraw_amount: u64 = 55;

        let data = WithdrawData::new(
            withdraw_amount,
            &keypair,
            current_balance,
            &current_ciphertext,
        )
        .unwrap();
        assert!(data.verify().is_ok());

        // generate and verify proof with wrong balance
        let wrong_balance: u64 = 99;
        let data = WithdrawData::new(
            withdraw_amount,
            &keypair,
            wrong_balance,
            &current_ciphertext,
        )
        .unwrap();
        assert!(data.verify().is_err());
    }
}
