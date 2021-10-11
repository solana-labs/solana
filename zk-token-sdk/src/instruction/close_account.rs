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

        // Edge case #1: if both C and D are identities, then this is a valid encryption of zero
        if C.is_identity() && D.is_identity() {
            transcript.append_point(b"R", &R);
            return Ok(());
        }

        // Edge case #2: if D is zeroed, but C is not, then this is an invalid ciphertext
        if D.is_identity() {
            transcript.append_point(b"R", &R);
            return Err(ProofError::VerificationError);
        }

        // generate a challenge scalar
        transcript.validate_and_append_point(b"R", &R)?;
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
    use super::*;
    use crate::encryption::elgamal::ElGamal;
    use crate::encryption::pedersen::{Pedersen, PedersenDecryptHandle, PedersenOpening};

    #[test]
    fn test_close_account_correctness() {
        let source = ElGamal::default();

        // invalid ciphertexts
        let balance = source.pk.encrypt(0_u64);

        let zeroed_comm = Pedersen::with(0_u64, &PedersenOpening::default());
        let handle = balance.decrypt_handle;

        let zeroed_comm_ciphertext = ElGamalCiphertext {
            message_comm: zeroed_comm,
            decrypt_handle: handle,
        };

        let proof = CloseAccountProof::new(&source.sk, &zeroed_comm_ciphertext);
        assert!(proof.verify(&zeroed_comm_ciphertext).is_err());

        let zeroed_handle_ciphertext = ElGamalCiphertext {
            message_comm: balance.message_comm,
            decrypt_handle: PedersenDecryptHandle::default(),
        };

        let proof = CloseAccountProof::new(&source.sk, &zeroed_handle_ciphertext);
        assert!(proof.verify(&zeroed_handle_ciphertext).is_err());

        // valid ciphertext, but encryption of non-zero amount
        let balance = source.pk.encrypt(55_u64);

        let proof = CloseAccountProof::new(&source.sk, &balance);
        assert!(proof.verify(&balance).is_err());

        // all-zeroed ciphertext interpretted as a valid encryption of zero
        let zeroed_ct: ElGamalCiphertext = pod::ElGamalCiphertext::zeroed().try_into().unwrap();
        let proof = CloseAccountProof::new(&source.sk, &zeroed_ct);
        assert!(proof.verify(&zeroed_ct).is_ok());

        // general case: valid encryption of zero
        let balance = source.pk.encrypt(0_u64);

        let proof = CloseAccountProof::new(&source.sk, &balance);
        assert!(proof.verify(&balance).is_ok());
    }
}
