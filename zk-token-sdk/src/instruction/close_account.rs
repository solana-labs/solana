use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::PedersenBase,
        },
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
    /// The source account ElGamal pubkey
    pub elgamal_pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The source account available balance in encrypted form
    pub balance: pod::ElGamalCiphertext, // 64 bytes

    /// Proof that the source account available balance is zero
    pub proof: CloseAccountProof, // 64 bytes
}

#[cfg(not(target_arch = "bpf"))]
impl CloseAccountData {
    pub fn new(source_keypair: &ElGamalKeypair, balance: ElGamalCiphertext) -> Self {
        let proof = CloseAccountProof::new(source_keypair, &balance);

        CloseAccountData {
            elgamal_pubkey: source_keypair.public.into(),
            balance: balance.into(),
            proof,
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for CloseAccountData {
    fn verify(&self) -> Result<(), ProofError> {
        let elgamal_pubkey = self.elgamal_pubkey.try_into()?;
        let balance = self.balance.try_into()?;
        self.proof.verify(&elgamal_pubkey, &balance)
    }
}

/// This struct represents the cryptographic proof component that certifies that the encrypted
/// balance is zero
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct CloseAccountProof {
    pub Y_P: pod::CompressedRistretto, // 32 bytes
    pub Y_D: pod::CompressedRistretto, // 32 bytes
    pub z: pod::Scalar,                // 32 bytes
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl CloseAccountProof {
    fn transcript_new() -> Transcript {
        Transcript::new(b"CloseAccountProof")
    }

    pub fn new(source_keypair: &ElGamalKeypair, balance: &ElGamalCiphertext) -> Self {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.close_account_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let P = source_keypair.public.get_point();
        let s = source_keypair.secret.get_scalar();

        let C = balance.message_comm.get_point();
        let D = balance.decrypt_handle.get_point();

        // record ElGamal pubkey and ciphertext in the transcript
        transcript.append_point(b"P", &P.compress());
        transcript.append_point(b"C", &C.compress());
        transcript.append_point(b"D", &D.compress());

        // generate a random masking factor that also serves as a nonce
        let y = Scalar::random(&mut OsRng);
        let Y_P = (y * P).compress();
        let Y_D = (y * D).compress();

        // record Y in transcript and receive a challenge scalar
        transcript.append_point(b"Y_P", &Y_P);
        transcript.append_point(b"Y_D", &Y_D);
        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // compute the masked secret key
        let z = c * s + y;

        CloseAccountProof {
            Y_P: Y_P.into(),
            Y_D: Y_D.into(),
            z: z.into(),
        }
    }

    pub fn verify(
        &self,
        elgamal_pubkey: &ElGamalPubkey,
        balance: &ElGamalCiphertext,
    ) -> Result<(), ProofError> {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.close_account_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let P = elgamal_pubkey.get_point();
        let C = balance.message_comm.get_point();
        let D = balance.decrypt_handle.get_point();

        let H = PedersenBase::default().H;

        let Y_P = self.Y_P.into();
        let Y_D = self.Y_D.into();
        let z = self.z.into();

        // record ElGamal pubkey and ciphertext in the transcript
        transcript.validate_and_append_point(b"P", &P.compress())?;
        transcript.append_point(b"C", &C.compress());
        transcript.append_point(b"D", &D.compress());

        // record Y in transcript and receive challenge scalars
        transcript.validate_and_append_point(b"Y_P", &Y_P)?;
        transcript.append_point(b"Y_D", &Y_D);

        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w"); // w used for multiscalar multiplication verification

        // decompress R or return verification error
        let Y_P = Y_P.decompress().ok_or(ProofError::VerificationError)?;
        let Y_D = Y_D.decompress().ok_or(ProofError::VerificationError)?;

        // check the required algebraic relation
        let check = RistrettoPoint::multiscalar_mul(
            vec![z, -c, -Scalar::one(), w * z, -w * c, -w],
            vec![P, H, Y_P, D, C, Y_D],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ProofError::VerificationError)
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{
            elgamal::ElGamalKeypair,
            pedersen::{Pedersen, PedersenDecryptHandle, PedersenOpening},
        },
    };

    #[test]
    fn test_close_account_correctness() {
        let source_keypair = ElGamalKeypair::default();

        // general case: encryption of 0
        let balance = source_keypair.public.encrypt(0_u64);
        let proof = CloseAccountProof::new(&source_keypair, &balance);
        assert!(proof.verify(&source_keypair.public, &balance).is_ok());

        // general case: encryption of > 0
        let balance = source_keypair.public.encrypt(1_u64);
        let proof = CloseAccountProof::new(&source_keypair, &balance);
        assert!(proof.verify(&source_keypair.public, &balance).is_err());

        // // edge case: all zero ciphertext - such ciphertext should always be a valid encryption of 0
        let zeroed_ct: ElGamalCiphertext = pod::ElGamalCiphertext::zeroed().try_into().unwrap();
        let proof = CloseAccountProof::new(&source_keypair, &zeroed_ct);
        assert!(proof.verify(&source_keypair.public, &zeroed_ct).is_ok());

        // edge cases: only C or D is zero - such ciphertext is always invalid
        let zeroed_comm = Pedersen::with(0_u64, &PedersenOpening::default());
        let handle = balance.decrypt_handle;

        let zeroed_comm_ciphertext = ElGamalCiphertext {
            message_comm: zeroed_comm,
            decrypt_handle: handle,
        };

        let proof = CloseAccountProof::new(&source_keypair, &zeroed_comm_ciphertext);
        assert!(proof
            .verify(&source_keypair.public, &zeroed_comm_ciphertext)
            .is_err());

        let zeroed_handle_ciphertext = ElGamalCiphertext {
            message_comm: balance.message_comm,
            decrypt_handle: PedersenDecryptHandle::default(),
        };

        let proof = CloseAccountProof::new(&source_keypair, &zeroed_handle_ciphertext);
        assert!(proof
            .verify(&source_keypair.public, &zeroed_handle_ciphertext)
            .is_err());
    }
}
