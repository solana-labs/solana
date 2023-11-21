#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamal, ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::{Pedersen, PedersenCommitment},
        },
        errors::ProofError,
        range_proof::RangeProof,
        sigma_proofs::ciphertext_commitment_equality_proof::CiphertextCommitmentEqualityProof,
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

#[cfg(not(target_os = "solana"))]
const WITHDRAW_AMOUNT_BIT_LENGTH: usize = 64;

/// The instruction data that is needed for the `ProofInstruction::VerifyWithdraw` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct WithdrawData {
    /// The context data for the withdraw proof
    pub context: WithdrawProofContext, // 128 bytes

    /// Range proof
    pub proof: WithdrawProof, // 736 bytes
}

/// The context data needed to verify a withdraw proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct WithdrawProofContext {
    /// The source account ElGamal pubkey
    pub pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The source account available balance *after* the withdraw (encrypted by
    /// `source_pk`
    pub final_ciphertext: pod::ElGamalCiphertext, // 64 bytes
}

#[cfg(not(target_os = "solana"))]
impl WithdrawData {
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

        let pod_pubkey = pod::ElGamalPubkey(keypair.pubkey().to_bytes());
        let pod_final_ciphertext: pod::ElGamalCiphertext = final_ciphertext.into();

        let context = WithdrawProofContext {
            pubkey: pod_pubkey,
            final_ciphertext: pod_final_ciphertext,
        };

        let mut transcript = context.new_transcript();
        let proof = WithdrawProof::new(keypair, final_balance, &final_ciphertext, &mut transcript);

        Ok(Self { context, proof })
    }
}

impl ZkProofData<WithdrawProofContext> for WithdrawData {
    const PROOF_TYPE: ProofType = ProofType::Withdraw;

    fn context_data(&self) -> &WithdrawProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context.new_transcript();

        let elgamal_pubkey = self.context.pubkey.try_into()?;
        let final_balance_ciphertext = self.context.final_ciphertext.try_into()?;
        self.proof
            .verify(&elgamal_pubkey, &final_balance_ciphertext, &mut transcript)
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl WithdrawProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"WithdrawProof");

        transcript.append_pubkey(b"pubkey", &self.pubkey);
        transcript.append_ciphertext(b"ciphertext", &self.final_ciphertext);

        transcript
    }
}

/// The withdraw proof.
///
/// It contains a ciphertext-commitment equality proof and a 64-bit range proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct WithdrawProof {
    /// New Pedersen commitment
    pub commitment: pod::PedersenCommitment,

    /// Associated equality proof
    pub equality_proof: pod::CiphertextCommitmentEqualityProof,

    /// Associated range proof
    pub range_proof: pod::RangeProofU64, // 672 bytes
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl WithdrawProof {
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
        let equality_proof = CiphertextCommitmentEqualityProof::new(
            keypair,
            final_ciphertext,
            &opening,
            final_balance,
            transcript,
        );

        let range_proof =
            RangeProof::new(vec![final_balance], vec![64], vec![&opening], transcript)
                .expect("range proof: generator");

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
        let equality_proof: CiphertextCommitmentEqualityProof = self.equality_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // verify equality proof
        equality_proof.verify(pubkey, final_ciphertext, &commitment, transcript)?;

        // verify range proof
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
        let current_ciphertext = keypair.pubkey().encrypt(current_balance);

        let withdraw_amount: u64 = 55;

        let data = WithdrawData::new(
            withdraw_amount,
            &keypair,
            current_balance,
            &current_ciphertext,
        )
        .unwrap();
        assert!(data.verify_proof().is_ok());

        // generate and verify proof with wrong balance
        let wrong_balance: u64 = 99;
        let data = WithdrawData::new(
            withdraw_amount,
            &keypair,
            wrong_balance,
            &current_ciphertext,
        )
        .unwrap();
        assert!(data.verify_proof().is_err());
    }
}
