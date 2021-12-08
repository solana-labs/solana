use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
        equality_proof::EqualityProof,
        errors::ProofError,
        instruction::Verifiable,
        range_proof::RangeProof,
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
pub struct WithdrawData {
    /// The source account ElGamal pubkey
    pub elgamal_pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The source account available balance *after* the withdraw (encrypted by
    /// `source_pk`
    pub final_balance_ct: pod::ElGamalCiphertext, // 64 bytes

    /// Range proof
    pub proof: WithdrawProof, // 736 bytes
}

impl WithdrawData {
    #[cfg(not(target_arch = "bpf"))]
    pub fn new(
        amount: u64,
        source_keypair: &ElGamalKeypair,
        current_balance: u64,
        current_balance_ct: ElGamalCiphertext,
    ) -> Self {
        // subtract withdraw amount from current balance
        //
        // panics if current_balance < amount
        let final_balance = current_balance - amount;

        // encode withdraw amount as an ElGamal ciphertext and subtract it from
        // current source balance
        let amount_encoded = source_keypair
            .public
            .encrypt_with(amount, &PedersenOpening::default());
        let final_balance_ct = current_balance_ct - amount_encoded;

        let proof = WithdrawProof::new(source_keypair, final_balance, &final_balance_ct);

        Self {
            elgamal_pubkey: source_keypair.public.into(),
            final_balance_ct: final_balance_ct.into(),
            proof,
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for WithdrawData {
    fn verify(&self) -> Result<(), ProofError> {
        let elgamal_pubkey = self.elgamal_pubkey.try_into()?;
        let final_balance_ct = self.final_balance_ct.try_into()?;
        self.proof.verify(&elgamal_pubkey, &final_balance_ct)
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
    pub equality_proof: pod::EqualityProof,

    /// Associated range proof
    pub range_proof: pod::RangeProof64, // 672 bytes
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl WithdrawProof {
    fn transcript_new() -> Transcript {
        Transcript::new(b"WithdrawProof")
    }

    pub fn new(
        source_keypair: &ElGamalKeypair,
        final_balance: u64,
        final_balance_ct: &ElGamalCiphertext,
    ) -> Self {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.withdraw_proof_domain_sep();

        // generate a Pedersen commitment for `final_balance`
        let (commitment, opening) = Pedersen::new(final_balance);

        // extract the relevant scalar and Ristretto points from the inputs
        let P_EG = source_keypair.public.get_point();
        let C_EG = final_balance_ct.message_comm.get_point();
        let D_EG = final_balance_ct.decrypt_handle.get_point();
        let C_Ped = commitment.get_point();

        // append all current state to the transcript
        transcript.append_point(b"P_EG", &P_EG.compress());
        transcript.append_point(b"C_EG", &C_EG.compress());
        transcript.append_point(b"D_EG", &D_EG.compress());
        transcript.append_point(b"C_Ped", &C_Ped.compress());

        // generate equality_proof
        let equality_proof = EqualityProof::new(
            source_keypair,
            final_balance_ct,
            final_balance,
            &opening,
            &mut transcript,
        );

        let range_proof = RangeProof::create(
            vec![final_balance],
            vec![64],
            vec![&opening],
            &mut transcript,
        );

        WithdrawProof {
            commitment: commitment.into(),
            equality_proof: equality_proof.try_into().expect("equality proof"),
            range_proof: range_proof.try_into().expect("range proof"),
        }
    }

    pub fn verify(
        &self,
        source_pk: &ElGamalPubkey,
        final_balance_ct: &ElGamalCiphertext,
    ) -> Result<(), ProofError> {
        let mut transcript = Self::transcript_new();

        let commitment: PedersenCommitment = self.commitment.try_into()?;
        let equality_proof: EqualityProof = self.equality_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // add a domain separator to record the start of the protocol
        transcript.withdraw_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the inputs
        let P_EG = source_pk.get_point();
        let C_EG = final_balance_ct.message_comm.get_point();
        let D_EG = final_balance_ct.decrypt_handle.get_point();
        let C_Ped = commitment.get_point();

        // append all current state to the transcript
        transcript.append_point(b"P_EG", &P_EG.compress());
        transcript.append_point(b"C_EG", &C_EG.compress());
        transcript.append_point(b"D_EG", &D_EG.compress());
        transcript.append_point(b"C_Ped", &C_Ped.compress());

        // verify equality proof
        //
        // TODO: we can also consider verifying equality and range proof in a batch
        equality_proof.verify(source_pk, final_balance_ct, &commitment, &mut transcript)?;

        // verify range proof
        //
        // TODO: double compressing here - consider modifying range proof input type to `PedersenCommitment`
        range_proof.verify(
            vec![&commitment.get_point().compress()],
            vec![64_usize],
            &mut transcript,
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
        let elgamal_keypair = ElGamalKeypair::default();

        let current_balance: u64 = 77;
        let current_balance_ct = elgamal_keypair.public.encrypt(current_balance);

        let withdraw_amount: u64 = 55;

        let data = WithdrawData::new(
            withdraw_amount,
            &elgamal_keypair,
            current_balance,
            current_balance_ct,
        );
        assert!(data.verify().is_ok());

        // generate and verify proof with wrong balance
        let wrong_balance: u64 = 99;
        let data = WithdrawData::new(
            withdraw_amount,
            &elgamal_keypair,
            wrong_balance,
            current_balance_ct,
        );
        assert!(data.verify().is_err());
    }
}
