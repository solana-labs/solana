#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::{
        elgamal::{ElGamalCiphertext, ElGamalPubkey},
        pedersen::{PedersenBase, PedersenCommitment, PedersenOpening},
    },
    rand::rngs::OsRng,
};
use {
    crate::{errors::ProofError, transcript::TranscriptProtocol},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, MultiscalarMul, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

#[derive(Clone)]
pub struct FeeProof {
    pub fee_max_proof: FeeMaxProof,
    pub fee_equality_proof: FeeEqualityProof,
}

#[allow(non_snake_case, dead_code)]
#[cfg(not(target_arch = "bpf"))]
impl FeeProof {
    pub fn new(
        amount_fee: u64,
        max_fee: u64,
        commitment_fee: PedersenCommitment,
        opening_fee: PedersenOpening,
        commitment_delta_real: PedersenCommitment,
        opening_delta_real: PedersenOpening,
        commitment_delta_claimed: PedersenCommitment,
        opening_delta_claimed: PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        // extract the relevant scalar and Ristretto points from the input
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;

        let x = Scalar::from(amount_fee);
        let m = Scalar::from(max_fee);

        let C_max = commitment_fee.get_point();
        let r_max = opening_fee.get_scalar();

        let C_delta_real = commitment_fee.get_point();
        let r_delta_real = opening_fee.get_scalar();

        let C_delta_claimed = commitment_fee.get_point();
        let r_delta_claimed = opening_fee.get_scalar();

        // record public values in transcript
        //
        // TODO: consider committing to these points outside this method
        transcript.append_point(b"C_max", &C_max.compress());
        transcript.append_point(b"C_delta_real", &C_delta_real.compress());
        transcript.append_point(b"C_delta_claimed", &C_delta_claimed.compress());

        // generate z values depending on whether the fee exceeds max fee or not
        //
        // TODO: use constant time conditional
        if amount_fee < max_fee {
            // simulate max proof
            let z_max = Scalar::random(&mut OsRng);
            let c_max = Scalar::random(&mut OsRng);
            let Y_max =
                RistrettoPoint::multiscalar_mul(vec![z_max, c_max, c_max * x], vec![H, C_max, G])
                    .compress();

            let fee_max_proof = FeeMaxProof {
                Y_max, z_max,
            };

            // generate equality proof
            let y_x = Scalar::random(&mut OsRng);
            let y_delta_real = Scalar::random(&mut OsRng);
            let y_delta_claimed = Scalar::random(&mut OsRng);

            let Y_delta_real =
                RistrettoPoint::multiscalar_mul(vec![y_x, y_delta_real], vec![G, H]).compress();
            let Y_delta_claimed =
                RistrettoPoint::multiscalar_mul(vec![y_x, y_delta_claimed], vec![G, H]).compress();

            transcript.append_point(b"Y_max", &Y_max);
            transcript.append_point(b"Y_delta_real", &Y_delta_real);
            transcript.append_point(b"Y_delta_claimed", &Y_delta_claimed);

            let c = transcript.challenge_scalar(b"c");
            let c_equality = c - c_max;

            let z_x = c_equality * x + y_x;
            let z_delta_real = c_equality * x + y_delta_real;
            let z_delta_claimed = c_equality * x + y_delta_claimed;

            let fee_equality_proof = FeeEqualityProof {
                Y_delta_real,
                Y_delta_claimed,
                z_x,
                z_delta_real,
                z_delta_claimed,
            };

            Self {
                fee_max_proof,
                fee_equality_proof,
            }

        } else {
            // simulate equality proof
            let z_x = Scalar::random(&mut OsRng);
            let z_delta_real = Scalar::random(&mut OsRng);
            let z_delta_claimed = Scalar::random(&mut OsRng);
            let c_equality = Scalar::random(&mut OsRng);

            let Y_delta_real = RistrettoPoint::multiscalar_mul(
                vec![z_x, z_delta_real, -c_equality],
                vec![G, H, C_delta_real],
            )
            .compress();

            let Y_delta_claimed = RistrettoPoint::multiscalar_mul(
                vec![z_x, z_delta_claimed, -c_equality],
                vec![G, H, C_delta_claimed],
            )
            .compress();

            let fee_equality_proof = FeeEqualityProof {
                Y_delta_real,
                Y_delta_claimed,
                z_x,
                z_delta_real,
                z_delta_claimed,
            };

            // generate max proof
            let y_max = Scalar::random(&mut OsRng);
            let Y_max = (y_max * H).compress();

            transcript.append_point(b"Y_max", &Y_max);
            transcript.append_point(b"Y_delta_real", &Y_delta_real);
            transcript.append_point(b"Y_delta_claimed", &Y_delta_claimed);

            let c = transcript.challenge_scalar(b"c");
            let c_max = c - c_equality;

            let z_max = c_max * r_max + y_max;

            let fee_max_proof = FeeMaxProof {
                Y_max, z_max,
            };

            Self {
                fee_max_proof,
                fee_equality_proof,
            }
        }
    }
}

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct FeeMaxProof {
    pub Y_max: CompressedRistretto,
    pub z_max: Scalar,
}

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct FeeEqualityProof {
    pub Y_delta_real: CompressedRistretto,
    pub Y_delta_claimed: CompressedRistretto,
    pub z_x: Scalar,
    pub z_delta_real: Scalar,
    pub z_delta_claimed: Scalar,
}

#[allow(non_snake_case, dead_code)]
#[cfg(not(target_arch = "bpf"))]
impl FeeProof {

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
