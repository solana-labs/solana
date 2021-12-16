#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::{
        elgamal::{ElGamalCiphertext, ElGamalPubkey},
        pedersen::{PedersenBase, PedersenOpening},
    },
    rand::rngs::OsRng,
};
use {
    crate::{errors::ProofError, transcript::TranscriptProtocol},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct FeeProof {
    pub Y_H: CompressedRistretto,
    pub Y_P: CompressedRistretto,
    pub z: Scalar,
}

#[allow(non_snake_case, dead_code)]
#[cfg(not(target_arch = "bpf"))]
impl FeeProof {
    pub fn new(
        fee_authority_elgamal_pubkey: &ElGamalPubkey,
        fee_ciphertext: &ElGamalCiphertext,
        max_fee: u64,
        opening: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        // extract the relevant scalar and Ristretto points from the input
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;
        let P = fee_authority_elgamal_pubkey.get_point();

        let m = Scalar::from(max_fee);
        let C = fee_ciphertext.message_comm.get_point() - m * G;
        let D = fee_ciphertext.decrypt_handle.get_point();
        let r = opening.get_scalar();

        // record public values in transcript
        //
        // TODO: consider committing to these points outside this method
        transcript.append_point(b"P", &P.compress());
        transcript.append_point(b"C", &C.compress());
        transcript.append_point(b"D", &D.compress());

        // generate a random masking factor that also serve as a nonce
        let y = Scalar::random(&mut OsRng);
        let Y_H = (y * H).compress();
        let Y_P = (y * P).compress();

        // record Y values in transcript and receive a challenge scalar
        transcript.append_point(b"Y_H", &Y_H);
        transcript.append_point(b"Y_P", &Y_P);
        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        println!("prover c: {:?}", c);

        // compute the masked encryption randomness
        let z = c * r + y;

        // TODO: actual fee calculation

        Self { Y_H, Y_P, z }
    }

    pub fn verify(
        self,
        fee_authority_elgamal_pubkey: &ElGamalPubkey,
        fee_ciphertext: &ElGamalCiphertext,
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        // extract the relevant scalar and Ristretto points from the inputs
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;
        let P = fee_authority_elgamal_pubkey.get_point();

        let m = Scalar::from(max_fee);
        let C = fee_ciphertext.message_comm.get_point() - m * G;
        let D = fee_ciphertext.decrypt_handle.get_point();

        // record public values in transcript
        transcript.append_point(b"P", &P.compress());
        transcript.append_point(b"C", &C.compress());
        transcript.append_point(b"D", &D.compress());

        transcript.validate_and_append_point(b"Y_H", &self.Y_H)?;
        transcript.validate_and_append_point(b"Y_P", &self.Y_P)?;

        // extract challenge scalars
        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w");

        println!("verifier c: {:?}", c);

        // check that the required algebraic condition holds
        let Y_H = self.Y_H.decompress().ok_or(ProofError::VerificationError)?;
        let Y_P = self.Y_P.decompress().ok_or(ProofError::VerificationError)?;

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![self.z, -c, -Scalar::one(), w * self.z, -w * c, -w],
            vec![H, C, Y_H, P, D, Y_P],
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
    use crate::encryption::elgamal::ElGamalKeypair;

    #[test]
    fn test_fee_proof() {
        // success case
        let fee_authority_keypair = ElGamalKeypair::default();
        let fee_authority_pubkey = fee_authority_keypair.public;
        let max_fee: u64 = 55;

        let opening = PedersenOpening::random(&mut OsRng);
        let fee_ciphertext = fee_authority_pubkey.encrypt_with(max_fee, &opening);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = FeeProof::new(
            &fee_authority_pubkey,
            &fee_ciphertext,
            max_fee,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &fee_authority_pubkey,
                &fee_ciphertext,
                max_fee,
                &mut transcript_verifier
            )
            .is_ok());
    }
}
