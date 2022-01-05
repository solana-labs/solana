#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::pedersen::{PedersenBase, PedersenCommitment, PedersenOpening},
    rand::rngs::OsRng,
};
use {
    crate::{sigma_proofs::errors::FeeProofError, transcript::TranscriptProtocol},
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        amount_fee: u64,
        max_fee: u64,
        delta_fee: u64,
        commitment_fee: &PedersenCommitment,
        opening_fee: &PedersenOpening,
        commitment_delta_real: &PedersenCommitment,
        opening_delta_real: &PedersenOpening,
        commitment_delta_claimed: &PedersenCommitment,
        opening_delta_claimed: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        // extract the relevant scalar and Ristretto points from the input
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;

        let x = Scalar::from(delta_fee);
        let m = Scalar::from(max_fee);

        let C_max = commitment_fee.get_point();
        let r_max = opening_fee.get_scalar();

        let C_delta_real = commitment_delta_real.get_point();
        let r_delta_real = opening_delta_real.get_scalar();

        let C_delta_claimed = commitment_delta_claimed.get_point();
        let r_delta_claimed = opening_delta_claimed.get_scalar();

        // record public values in transcript
        //
        // TODO: consider committing to these points outside this method
        transcript.append_point(b"C_max", &C_max.compress());
        transcript.append_point(b"C_delta_real", &C_delta_real.compress());
        transcript.append_point(b"C_delta_claimed", &C_delta_claimed.compress());

        // generate z values depending on whether the fee exceeds max fee or not
        //
        // TODO: must implement this for constant time
        if amount_fee < max_fee {
            // simulate max proof
            let z_max = Scalar::random(&mut OsRng);
            let c_max = Scalar::random(&mut OsRng);
            let Y_max =
                RistrettoPoint::multiscalar_mul(vec![z_max, -c_max, c_max * m], vec![H, C_max, G])
                    .compress();

            let fee_max_proof = FeeMaxProof {
                Y_max,
                z_max,
                c_max,
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

            transcript.challenge_scalar(b"w");

            let z_x = c_equality * x + y_x;
            let z_delta_real = c_equality * r_delta_real + y_delta_real;
            let z_delta_claimed = c_equality * r_delta_claimed + y_delta_claimed;

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

            transcript.challenge_scalar(b"w");

            let z_max = c_max * r_max + y_max;

            let fee_max_proof = FeeMaxProof {
                Y_max,
                z_max,
                c_max,
            };

            Self {
                fee_max_proof,
                fee_equality_proof,
            }
        }
    }

    pub fn verify(
        self,
        max_fee: u64,
        commitment_fee: PedersenCommitment,
        commitment_delta_real: PedersenCommitment,
        commitment_delta_claimed: PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Result<(), FeeProofError> {
        // extract the relevant scalar and Ristretto points from the input
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;

        let m = Scalar::from(max_fee);

        let C_max = commitment_fee.get_point();
        let C_delta_real = commitment_delta_real.get_point();
        let C_delta_claimed = commitment_delta_claimed.get_point();

        // record public values in transcript
        //
        // TODO: consider committing to these points outside this method
        transcript.validate_and_append_point(b"C_max", &C_max.compress())?;
        transcript.validate_and_append_point(b"C_delta_real", &C_delta_real.compress())?;
        transcript.validate_and_append_point(b"C_delta_claimed", &C_delta_claimed.compress())?;

        transcript.validate_and_append_point(b"Y_max", &self.fee_max_proof.Y_max)?;
        transcript
            .validate_and_append_point(b"Y_delta_real", &self.fee_equality_proof.Y_delta_real)?;
        transcript.validate_and_append_point(
            b"Y_delta_claimed",
            &self.fee_equality_proof.Y_delta_claimed,
        )?;

        let Y_max = self
            .fee_max_proof
            .Y_max
            .decompress()
            .ok_or(FeeProofError::Format)?;
        let z_max = self.fee_max_proof.z_max;

        let Y_delta_real = self
            .fee_equality_proof
            .Y_delta_real
            .decompress()
            .ok_or(FeeProofError::Format)?;
        let Y_delta_claimed = self
            .fee_equality_proof
            .Y_delta_claimed
            .decompress()
            .ok_or(FeeProofError::Format)?;
        let z_x = self.fee_equality_proof.z_x;
        let z_delta_real = self.fee_equality_proof.z_delta_real;
        let z_delta_claimed = self.fee_equality_proof.z_delta_claimed;

        let c = transcript.challenge_scalar(b"c");
        let c_max = self.fee_max_proof.c_max;
        let c_equality = c - c_max;

        let w = transcript.challenge_scalar(b"w");
        let ww = w * w;

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![
                c_max,
                -c_max * m,
                -z_max,
                Scalar::one(),
                w * z_x,
                w * z_delta_real,
                -w * c_equality,
                -w,
                ww * z_x,
                ww * z_delta_claimed,
                -ww * c_equality,
                -ww,
            ],
            vec![
                C_max,
                G,
                H,
                Y_max,
                G,
                H,
                C_delta_real,
                Y_delta_real,
                G,
                H,
                C_delta_claimed,
                Y_delta_claimed,
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(FeeProofError::AlgebraicRelation)
        }
    }
}

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct FeeMaxProof {
    pub Y_max: CompressedRistretto,
    pub z_max: Scalar,
    pub c_max: Scalar,
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::encryption::pedersen::Pedersen;

    #[test]
    fn test_fee_proof() {
        let transfer_amount: u64 = 55;
        let max_fee: u64 = 77;

        let rate_fee: u16 = 555; // 5.55%
        let amount_fee = 3;
        let delta_fee: u64 = 525;

        let (commitment_transfer, opening_transfer) = Pedersen::new(transfer_amount);
        let (commitment_fee, opening_fee) = Pedersen::new(amount_fee);

        let scalar_rate = Scalar::from(rate_fee);
        let commitment_delta_real =
            commitment_transfer * scalar_rate - commitment_fee * Scalar::from(10000_u64);
        let opening_delta_real =
            opening_transfer * scalar_rate - opening_fee.clone() * Scalar::from(10000_u64);

        let (commitment_delta_claimed, opening_delta_claimed) = Pedersen::new(delta_fee);

        let mut transcript_prover = Transcript::new(b"test");
        let mut transcript_verifier = Transcript::new(b"test");

        let proof = FeeProof::new(
            amount_fee,
            max_fee,
            delta_fee,
            &commitment_fee,
            &opening_fee,
            &commitment_delta_real,
            &opening_delta_real,
            &commitment_delta_claimed,
            &opening_delta_claimed,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                max_fee,
                commitment_fee,
                commitment_delta_real,
                commitment_delta_claimed,
                &mut transcript_verifier,
            )
            .is_ok());
    }
}
