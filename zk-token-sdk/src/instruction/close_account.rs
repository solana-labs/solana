use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::elgamal::{ElGamalCiphertext, ElGamalSecretKey},
        errors::ProofError,
        instruction::Verifiable,
        transcript::TranscriptProtocol,
    },
    curve25519_dalek::{
        ristretto::RistrettoPoint,
        scalar::Scalar,
        traits::{IsIdentity, MultiscalarMul},
    },
    merlin::Transcript,
    rand::rngs::OsRng,
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
    /// The source account available balance in encrypted form
    pub balance: pod::ElGamalCiphertext, // 64 bytes

    /// Proof that the source account available balance is zero
    pub proof: CloseAccountProof, // 64 bytes
}

#[cfg(not(target_arch = "bpf"))]
impl CloseAccountData {
    pub fn new(source_sk: &ElGamalSecretKey, balance: ElGamalCiphertext) -> Self {
        let proof = CloseAccountProof::new(source_sk, &balance);

        CloseAccountData {
            balance: balance.into(),
            proof,
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for CloseAccountData {
    fn verify(&self) -> Result<(), ProofError> {
        let balance = self.balance.try_into()?;
        self.proof.verify(&balance)
    }
}

/// This struct represents the cryptographic proof component that certifies that the encrypted
/// balance is zero
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct CloseAccountProof {
    pub R: pod::CompressedRistretto, // 32 bytes
    pub z: pod::Scalar,              // 32 bytes
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl CloseAccountProof {
    fn transcript_new() -> Transcript {
        Transcript::new(b"CloseAccountProof")
    }

    pub fn new(source_sk: &ElGamalSecretKey, balance: &ElGamalCiphertext) -> Self {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.close_account_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let s = source_sk.get_scalar();
        let C = balance.decrypt_handle.get_point();

        // generate a random masking factor that also serves as a nonce
        let r = Scalar::random(&mut OsRng); // using OsRng for now
        let R = (r * C).compress();

        // record R on transcript and receive a challenge scalar
        transcript.append_point(b"R", &R);
        let c = transcript.challenge_scalar(b"c");

        // compute the masked secret key
        let z = c * s + r;

        CloseAccountProof {
            R: R.into(),
            z: z.into(),
        }
    }

    pub fn verify(&self, balance: &ElGamalCiphertext) -> Result<(), ProofError> {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.close_account_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let C = balance.message_comm.get_point();
        let D = balance.decrypt_handle.get_point();

        let R = self.R.into();
        let z = self.z.into();

        // generate a challenge scalar
        //
        // use `append_point` as opposed to `validate_and_append_point` as the ciphertext is
        // already guaranteed to be well-formed
        transcript.append_point(b"R", &R);
        let c = transcript.challenge_scalar(b"c");

        // decompress R or return verification error
        let R = R.decompress().ok_or(ProofError::VerificationError)?;

        // check the required algebraic relation
        let check = RistrettoPoint::multiscalar_mul(vec![z, -c, -Scalar::one()], vec![D, C, R]);

        if check.is_identity() {
            Ok(())
        } else {
            Err(ProofError::VerificationError)
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::elgamal::ElGamal};

    #[test]
    fn test_close_account_correctness() {
        let (source_pk, source_sk) = ElGamal::new();

        // If account balance is 0, then the proof should succeed
        let balance = source_pk.encrypt(0_u64);

        let proof = CloseAccountProof::new(&source_sk, &balance);
        assert!(proof.verify(&balance).is_ok());

        // If account balance is not zero, then the proof verification should fail
        let balance = source_pk.encrypt(55_u64);

        let proof = CloseAccountProof::new(&source_sk, &balance);
        assert!(proof.verify(&balance).is_err());

        // A zeroed cyphertext should be considered as an account balance of 0
        let zeroed_ct: ElGamalCiphertext = pod::ElGamalCiphertext::zeroed().try_into().unwrap();
        let proof = CloseAccountProof::new(&source_sk, &zeroed_ct);
        assert!(proof.verify(&zeroed_ct).is_ok());
    }
}
