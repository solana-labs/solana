use {
    crate::pod::*,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCT, ElGamalPK, ElGamalSK},
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
/// - The pre-instruction should call UpdateAccountPKData::verify(&self)
/// - The actual program should check that `current_ct` is consistent with what is
///   currently stored in the confidential token account
///
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct UpdateAccountPkData {
    /// Current ElGamal encryption key
    pub current_pk: PodElGamalPK, // 32 bytes

    /// Current encrypted available balance
    pub current_ct: PodElGamalCT, // 64 bytes

    /// New ElGamal encryption key
    pub new_pk: PodElGamalPK, // 32 bytes

    /// New encrypted available balance
    pub new_ct: PodElGamalCT, // 64 bytes

    /// Proof that the current and new ciphertexts are consistent
    pub proof: UpdateAccountPkProof, // 160 bytes
}

impl UpdateAccountPkData {
    #[cfg(not(target_arch = "bpf"))]
    pub fn new(
        current_balance: u64,
        current_ct: ElGamalCT,
        current_pk: ElGamalPK,
        current_sk: &ElGamalSK,
        new_pk: ElGamalPK,
        new_sk: &ElGamalSK,
    ) -> Self {
        let new_ct = new_pk.encrypt(current_balance);

        let proof =
            UpdateAccountPkProof::new(current_balance, current_sk, new_sk, &current_ct, &new_ct);

        Self {
            current_pk: current_pk.into(),
            current_ct: current_ct.into(),
            new_ct: new_ct.into(),
            new_pk: new_pk.into(),
            proof,
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for UpdateAccountPkData {
    fn verify(&self) -> Result<(), ProofError> {
        let current_ct = self.current_ct.try_into()?;
        let new_ct = self.new_ct.try_into()?;
        self.proof.verify(&current_ct, &new_ct)
    }
}

/// This struct represents the cryptographic proof component that certifies that the current_ct and
/// new_ct encrypt equal values
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct UpdateAccountPkProof {
    pub R_0: PodCompressedRistretto, // 32 bytes
    pub R_1: PodCompressedRistretto, // 32 bytes
    pub z_sk_0: PodScalar,           // 32 bytes
    pub z_sk_1: PodScalar,           // 32 bytes
    pub z_x: PodScalar,              // 32 bytes
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl UpdateAccountPkProof {
    fn transcript_new() -> Transcript {
        Transcript::new(b"UpdateAccountPkProof")
    }

    fn new(
        current_balance: u64,
        current_sk: &ElGamalSK,
        new_sk: &ElGamalSK,
        current_ct: &ElGamalCT,
        new_ct: &ElGamalCT,
    ) -> Self {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.update_account_public_key_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let s_0 = current_sk.get_scalar();
        let s_1 = new_sk.get_scalar();
        let x = Scalar::from(current_balance);

        let D_0 = current_ct.decrypt_handle.get_point();
        let D_1 = new_ct.decrypt_handle.get_point();

        let G = PedersenBase::default().G;

        // generate a random masking factor that also serves as a nonce
        let r_sk_0 = Scalar::random(&mut OsRng);
        let r_sk_1 = Scalar::random(&mut OsRng);
        let r_x = Scalar::random(&mut OsRng);

        let R_0 = (r_sk_0 * D_0 + r_x * G).compress();
        let R_1 = (r_sk_1 * D_1 + r_x * G).compress();

        // record R_0, R_1 on transcript and receive a challenge scalar
        transcript.append_point(b"R_0", &R_0);
        transcript.append_point(b"R_1", &R_1);
        let c = transcript.challenge_scalar(b"c");
        let _w = transcript.challenge_scalar(b"w"); // for consistency of transcript

        // compute the masked secret keys and amount
        let z_sk_0 = c * s_0 + r_sk_0;
        let z_sk_1 = c * s_1 + r_sk_1;
        let z_x = c * x + r_x;

        UpdateAccountPkProof {
            R_0: R_0.into(),
            R_1: R_1.into(),
            z_sk_0: z_sk_0.into(),
            z_sk_1: z_sk_1.into(),
            z_x: z_x.into(),
        }
    }

    fn verify(&self, current_ct: &ElGamalCT, new_ct: &ElGamalCT) -> Result<(), ProofError> {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.update_account_public_key_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let C_0 = current_ct.message_comm.get_point();
        let D_0 = current_ct.decrypt_handle.get_point();

        let C_1 = new_ct.message_comm.get_point();
        let D_1 = new_ct.decrypt_handle.get_point();

        let R_0 = self.R_0.into();
        let R_1 = self.R_1.into();
        let z_sk_0 = self.z_sk_0.into();
        let z_sk_1: Scalar = self.z_sk_1.into();
        let z_x = self.z_x.into();

        let G = PedersenBase::default().G;

        // generate a challenge scalar
        transcript.validate_and_append_point(b"R_0", &R_0)?;
        transcript.validate_and_append_point(b"R_1", &R_1)?;
        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w");

        // decompress R_0, R_1 or return verification error
        let R_0 = R_0.decompress().ok_or(ProofError::VerificationError)?;
        let R_1 = R_1.decompress().ok_or(ProofError::VerificationError)?;

        // check the required algebraic relation
        let check = RistrettoPoint::multiscalar_mul(
            vec![
                z_sk_0,
                z_x,
                -c,
                -Scalar::one(),
                w * z_sk_1,
                w * z_x,
                -w * c,
                -w * Scalar::one(),
            ],
            vec![D_0, G, C_0, R_0, D_1, G, C_1, R_1],
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
    use super::*;
    use crate::encryption::elgamal::ElGamal;

    #[test]
    fn test_update_account_public_key_correctness() {
        let (current_pk, current_sk) = ElGamal::keygen();
        let (new_pk, new_sk) = ElGamal::keygen();

        // If current_ct and new_ct encrypt same values, then the proof verification should succeed
        let balance: u64 = 77;
        let current_ct = current_pk.encrypt(balance);
        let new_ct = new_pk.encrypt(balance);

        let proof = UpdateAccountPkProof::new(balance, &current_sk, &new_sk, &current_ct, &new_ct);
        assert!(proof.verify(&current_ct, &new_ct).is_ok());

        // If current_ct and new_ct encrypt different values, then the proof verification should fail
        let new_ct = new_pk.encrypt(55_u64);

        let proof = UpdateAccountPkProof::new(balance, &current_sk, &new_sk, &current_ct, &new_ct);
        assert!(proof.verify(&current_ct, &new_ct).is_err());

        // A zeroed cipehrtext should be considered as an account balance of 0
        let balance: u64 = 0;
        let zeroed_ct_as_current_ct: ElGamalCT = PodElGamalCT::zeroed().try_into().unwrap();
        let new_ct: ElGamalCT = new_pk.encrypt(balance);
        let proof = UpdateAccountPkProof::new(
            balance,
            &current_sk,
            &new_sk,
            &zeroed_ct_as_current_ct,
            &new_ct,
        );
        assert!(proof.verify(&zeroed_ct_as_current_ct, &new_ct).is_ok());

        let current_ct: ElGamalCT = PodElGamalCT::zeroed().try_into().unwrap();
        let zeroed_ct_as_new_ct: ElGamalCT = PodElGamalCT::zeroed().try_into().unwrap();
        let proof = UpdateAccountPkProof::new(
            balance,
            &current_sk,
            &new_sk,
            &current_ct,
            &zeroed_ct_as_new_ct,
        );
        assert!(proof.verify(&current_ct, &zeroed_ct_as_new_ct).is_ok());

        let zeroed_ct_as_current_ct: ElGamalCT = PodElGamalCT::zeroed().try_into().unwrap();
        let zeroed_ct_as_new_ct: ElGamalCT = PodElGamalCT::zeroed().try_into().unwrap();
        let proof = UpdateAccountPkProof::new(
            balance,
            &current_sk,
            &new_sk,
            &zeroed_ct_as_current_ct,
            &zeroed_ct_as_new_ct,
        );
        assert!(proof
            .verify(&zeroed_ct_as_current_ct, &zeroed_ct_as_new_ct)
            .is_ok());
    }
}
