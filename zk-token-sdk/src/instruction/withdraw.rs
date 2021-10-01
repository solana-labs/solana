use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalPubkey, ElGamalSecretKey},
            pedersen::{PedersenBase, PedersenOpen},
        },
        errors::ProofError,
        instruction::Verifiable,
        range_proof::RangeProof,
        transcript::TranscriptProtocol,
    },
    curve25519_dalek::{ristretto::RistrettoPoint, scalar::Scalar, traits::MultiscalarMul},
    merlin::Transcript,
    rand::rngs::OsRng,
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
    /// The source account available balance *after* the withdraw (encrypted by
    /// `source_pk`
    pub final_balance_ct: pod::ElGamalCiphertext, // 64 bytes

    /// Proof that the account is solvent
    pub proof: WithdrawProof, // 736 bytes
}

impl WithdrawData {
    #[cfg(not(target_arch = "bpf"))]
    pub fn new(
        amount: u64,
        source_pk: ElGamalPubkey,
        source_sk: &ElGamalSecretKey,
        current_balance: u64,
        current_balance_ct: ElGamalCiphertext,
    ) -> Self {
        // subtract withdraw amount from current balance
        //
        // panics if current_balance < amount
        let final_balance = current_balance - amount;

        // encode withdraw amount as an ElGamal ciphertext and subtract it from
        // current source balance
        let amount_encoded = source_pk.encrypt_with(amount, &PedersenOpen::default());
        let final_balance_ct = current_balance_ct - amount_encoded;

        let proof = WithdrawProof::new(source_sk, final_balance, &final_balance_ct);

        Self {
            final_balance_ct: final_balance_ct.into(),
            proof,
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for WithdrawData {
    fn verify(&self) -> Result<(), ProofError> {
        let final_balance_ct = self.final_balance_ct.try_into()?;
        self.proof.verify(&final_balance_ct)
    }
}

/// This struct represents the cryptographic proof component that certifies the account's solvency
/// for withdrawal
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct WithdrawProof {
    /// Wrapper for range proof: R component
    pub R: pod::CompressedRistretto, // 32 bytes
    /// Wrapper for range proof: z component
    pub z: pod::Scalar, // 32 bytes
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
        source_sk: &ElGamalSecretKey,
        final_balance: u64,
        final_balance_ct: &ElGamalCiphertext,
    ) -> Self {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.withdraw_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let H = PedersenBase::default().H;
        let D = final_balance_ct.decrypt_handle.get_point();
        let s = source_sk.get_scalar();

        // new pedersen opening
        let r_new = Scalar::random(&mut OsRng);

        // generate a random masking factor that also serves as a nonce
        let y = Scalar::random(&mut OsRng);

        let R = RistrettoPoint::multiscalar_mul(vec![y, r_new], vec![D, H]).compress();

        // record R on transcript and receive a challenge scalar
        transcript.append_point(b"R", &R);
        let c = transcript.challenge_scalar(b"c");

        // compute the masked secret key
        let z = s + c * y;

        // compute the new Pedersen commitment and opening
        let new_open = PedersenOpen(c * r_new);

        let range_proof = RangeProof::create(
            vec![final_balance],
            vec![64],
            vec![&new_open],
            &mut transcript,
        );

        WithdrawProof {
            R: R.into(),
            z: z.into(),
            range_proof: range_proof.try_into().expect("range proof"),
        }
    }

    pub fn verify(&self, final_balance_ct: &ElGamalCiphertext) -> Result<(), ProofError> {
        let mut transcript = Self::transcript_new();

        // Add a domain separator to record the start of the protocol
        transcript.withdraw_proof_domain_sep();

        // Extract the relevant scalar and Ristretto points from the input
        let C = final_balance_ct.message_comm.get_point();
        let D = final_balance_ct.decrypt_handle.get_point();

        let R = self.R.into();
        let z: Scalar = self.z.into();

        // generate a challenge scalar
        transcript.validate_and_append_point(b"R", &R)?;
        let c = transcript.challenge_scalar(b"c");

        // decompress R or return verification error
        let R = R.decompress().ok_or(ProofError::VerificationError)?;

        // compute new Pedersen commitment to verify range proof with
        let new_comm = RistrettoPoint::multiscalar_mul(vec![Scalar::one(), -z, c], vec![C, D, R]);

        let range_proof: RangeProof = self.range_proof.try_into()?;
        range_proof.verify(vec![&new_comm.compress()], vec![64_usize], &mut transcript)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encryption::elgamal::ElGamal;

    #[test]
    #[ignore]
    fn test_withdraw_correctness() {
        // generate and verify proof for the proper setting
        let (source_pk, source_sk) = ElGamal::keygen();

        let current_balance: u64 = 77;
        let current_balance_ct = source_pk.encrypt(current_balance);

        let withdraw_amount: u64 = 55;

        let data = WithdrawData::new(
            withdraw_amount,
            source_pk,
            &source_sk,
            current_balance,
            current_balance_ct,
        );
        assert!(data.verify().is_ok());

        // generate and verify proof with wrong balance
        let wrong_balance: u64 = 99;
        let data = WithdrawData::new(
            withdraw_amount,
            source_pk,
            &source_sk,
            wrong_balance,
            current_balance_ct,
        );
        assert!(data.verify().is_err());

        // TODO: test for ciphertexts that encrypt numbers outside the 0, 2^64 range
    }
}
