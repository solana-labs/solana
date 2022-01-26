#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::pedersen::{PedersenCommitment, PedersenOpening, G, H},
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
    subtle::{Choice, ConditionallySelectable, ConstantTimeGreater},
};

#[derive(Clone)]
pub struct FeeProof {
    pub fee_max_proof: FeeMaxProof,
    pub fee_equality_proof: FeeEqualityProof,
}

#[allow(non_snake_case, dead_code)]
#[cfg(not(target_arch = "bpf"))]
impl FeeProof {
    fn create_proof_fee_above_max(
        opening_fee: &PedersenOpening,
        commitment_delta: &PedersenCommitment,
        commitment_delta_claimed: &PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Self {
        // simulate equality proof
        let C_delta = commitment_delta.get_point();
        let C_delta_claimed = commitment_delta_claimed.get_point();

        let z_x = Scalar::random(&mut OsRng);
        let z_delta = Scalar::random(&mut OsRng);
        let z_delta_claimed = Scalar::random(&mut OsRng);
        let c_equality = Scalar::random(&mut OsRng);

        let Y_delta = RistrettoPoint::multiscalar_mul(
            vec![z_x, z_delta, -c_equality],
            vec![&(*G), &(*H), C_delta],
        )
        .compress();

        let Y_delta_claimed = RistrettoPoint::multiscalar_mul(
            vec![z_x, z_delta_claimed, -c_equality],
            vec![&(*G), &(*H), C_delta_claimed],
        )
        .compress();

        let fee_equality_proof = FeeEqualityProof {
            Y_delta,
            Y_delta_claimed,
            z_x,
            z_delta,
            z_delta_claimed,
        };

        // generate max proof
        let r_fee = opening_fee.get_scalar();

        let y_max_proof = Scalar::random(&mut OsRng);
        let Y_max_proof = (y_max_proof * &(*H)).compress();

        transcript.append_point(b"Y_max_proof", &Y_max_proof);
        transcript.append_point(b"Y_delta", &Y_delta);
        transcript.append_point(b"Y_delta_claimed", &Y_delta_claimed);

        let c = transcript.challenge_scalar(b"c");
        let c_max_proof = c - c_equality;

        transcript.challenge_scalar(b"w");

        let z_max_proof = c_max_proof * r_fee + y_max_proof;

        let fee_max_proof = FeeMaxProof {
            Y_max_proof,
            z_max_proof,
            c_max_proof,
        };

        Self {
            fee_max_proof,
            fee_equality_proof,
        }
    }

    fn create_proof_fee_below_max(
        commitment_fee: &PedersenCommitment,
        (delta_fee, opening_delta): (u64, &PedersenOpening),
        opening_delta_claimed: &PedersenOpening,
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Self {
        // simulate max proof
        let m = Scalar::from(max_fee);
        let C_fee = commitment_fee.get_point();

        let z_max_proof = Scalar::random(&mut OsRng);
        let c_max_proof = Scalar::random(&mut OsRng); // random challenge

        // solve for Y_max in the verification algebraic relation
        let Y_max_proof = RistrettoPoint::multiscalar_mul(
            vec![z_max_proof, -c_max_proof, c_max_proof * m],
            vec![&(*H), C_fee, &(*G)],
        )
        .compress();

        let fee_max_proof = FeeMaxProof {
            Y_max_proof,
            z_max_proof,
            c_max_proof,
        };

        // generate equality proof
        let x = Scalar::from(delta_fee);

        let r_delta = opening_delta.get_scalar();
        let r_delta_claimed = opening_delta_claimed.get_scalar();

        let y_x = Scalar::random(&mut OsRng);
        let y_delta = Scalar::random(&mut OsRng);
        let y_delta_claimed = Scalar::random(&mut OsRng);

        let Y_delta =
            RistrettoPoint::multiscalar_mul(vec![y_x, y_delta], vec![&(*G), &(*H)]).compress();
        let Y_delta_claimed =
            RistrettoPoint::multiscalar_mul(vec![y_x, y_delta_claimed], vec![&(*G), &(*H)])
                .compress();

        transcript.append_point(b"Y_max_proof", &Y_max_proof);
        transcript.append_point(b"Y_delta", &Y_delta);
        transcript.append_point(b"Y_delta_claimed", &Y_delta_claimed);

        let c = transcript.challenge_scalar(b"c");
        let c_equality = c - c_max_proof;

        transcript.challenge_scalar(b"w");

        let z_x = c_equality * x + y_x;
        let z_delta = c_equality * r_delta + y_delta;
        let z_delta_claimed = c_equality * r_delta_claimed + y_delta_claimed;

        let fee_equality_proof = FeeEqualityProof {
            Y_delta,
            Y_delta_claimed,
            z_x,
            z_delta,
            z_delta_claimed,
        };

        Self {
            fee_max_proof,
            fee_equality_proof,
        }
    }

    pub fn new(
        (fee_amount, commitment_fee, opening_fee): (u64, &PedersenCommitment, &PedersenOpening),
        (delta_fee, commitment_delta, opening_delta): (u64, &PedersenCommitment, &PedersenOpening),
        (commitment_delta_claimed, opening_delta_claimed): (&PedersenCommitment, &PedersenOpening),
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Self {
        let mut transcript_fee_above_max = transcript.clone();
        let mut transcript_fee_below_max = transcript.clone();

        // compute proof for both cases `fee_amount' >= `max_fee` and `fee_amount` < `max_fee`
        let proof_fee_above_max = Self::create_proof_fee_above_max(
            opening_fee,
            commitment_delta,
            commitment_delta_claimed,
            &mut transcript_fee_above_max,
        );

        let proof_fee_below_max = Self::create_proof_fee_below_max(
            commitment_fee,
            (delta_fee, opening_delta),
            opening_delta_claimed,
            max_fee,
            &mut transcript_fee_below_max,
        );

        let above_max = u64::ct_gt(&max_fee, &fee_amount);

        // conditionally assign transcript; transcript is not conditionally selectable
        if bool::from(above_max) {
            *transcript = transcript_fee_above_max;
        } else {
            *transcript = transcript_fee_below_max;
        }

        Self {
            fee_max_proof: FeeMaxProof::conditional_select(
                &proof_fee_above_max.fee_max_proof,
                &proof_fee_below_max.fee_max_proof,
                above_max,
            ),
            fee_equality_proof: FeeEqualityProof::conditional_select(
                &proof_fee_above_max.fee_equality_proof,
                &proof_fee_below_max.fee_equality_proof,
                above_max,
            ),
        }
    }

    pub fn verify(
        self,
        max_fee: u64,
        commitment_fee: &PedersenCommitment,
        commitment_delta: &PedersenCommitment,
        commitment_delta_claimed: &PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Result<(), FeeProofError> {
        // extract the relevant scalar and Ristretto points from the input
        let m = Scalar::from(max_fee);

        let C_max = commitment_fee.get_point();
        let C_delta = commitment_delta.get_point();
        let C_delta_claimed = commitment_delta_claimed.get_point();

        transcript.validate_and_append_point(b"Y_max_proof", &self.fee_max_proof.Y_max_proof)?;
        transcript.validate_and_append_point(b"Y_delta", &self.fee_equality_proof.Y_delta)?;
        transcript.validate_and_append_point(
            b"Y_delta_claimed",
            &self.fee_equality_proof.Y_delta_claimed,
        )?;

        let Y_max = self
            .fee_max_proof
            .Y_max_proof
            .decompress()
            .ok_or(FeeProofError::Format)?;
        let z_max = self.fee_max_proof.z_max_proof;

        let Y_delta_real = self
            .fee_equality_proof
            .Y_delta
            .decompress()
            .ok_or(FeeProofError::Format)?;
        let Y_delta_claimed = self
            .fee_equality_proof
            .Y_delta_claimed
            .decompress()
            .ok_or(FeeProofError::Format)?;
        let z_x = self.fee_equality_proof.z_x;
        let z_delta_real = self.fee_equality_proof.z_delta;
        let z_delta_claimed = self.fee_equality_proof.z_delta_claimed;

        let c = transcript.challenge_scalar(b"c");
        let c_max_proof = self.fee_max_proof.c_max_proof;
        let c_equality = c - c_max_proof;

        let w = transcript.challenge_scalar(b"w");
        let ww = w * w;

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![
                c_max_proof,
                -c_max_proof * m,
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
                &(*G),
                &(*H),
                &Y_max,
                &(*G),
                &(*H),
                C_delta,
                &Y_delta_real,
                &(*G),
                &(*H),
                C_delta_claimed,
                &Y_delta_claimed,
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(FeeProofError::AlgebraicRelation)
        }
    }
}

/// TODO: mention copy
#[allow(non_snake_case)]
#[derive(Clone, Copy)]
pub struct FeeMaxProof {
    pub Y_max_proof: CompressedRistretto,
    pub z_max_proof: Scalar,
    pub c_max_proof: Scalar,
}

impl ConditionallySelectable for FeeMaxProof {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        Self {
            Y_max_proof: conditional_select_ristretto(&a.Y_max_proof, &b.Y_max_proof, choice),
            z_max_proof: Scalar::conditional_select(&a.z_max_proof, &b.z_max_proof, choice),
            c_max_proof: Scalar::conditional_select(&a.c_max_proof, &b.c_max_proof, choice),
        }
    }
}

fn conditional_select_ristretto(
    a: &CompressedRistretto,
    b: &CompressedRistretto,
    choice: Choice,
) -> CompressedRistretto {
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        bytes[i] = u8::conditional_select(&a.as_bytes()[i], &b.as_bytes()[i], choice);
    }
    CompressedRistretto(bytes)
}

/// TODO: mention copy
#[allow(non_snake_case)]
#[derive(Clone, Copy)]
pub struct FeeEqualityProof {
    pub Y_delta: CompressedRistretto,
    pub Y_delta_claimed: CompressedRistretto,
    pub z_x: Scalar,
    pub z_delta: Scalar,
    pub z_delta_claimed: Scalar,
}

impl ConditionallySelectable for FeeEqualityProof {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        Self {
            Y_delta: conditional_select_ristretto(&a.Y_delta, &b.Y_delta, choice),
            Y_delta_claimed: conditional_select_ristretto(
                &a.Y_delta_claimed,
                &b.Y_delta_claimed,
                choice,
            ),
            z_x: Scalar::conditional_select(&a.z_x, &b.z_x, choice),
            z_delta: Scalar::conditional_select(&a.z_delta, &b.z_delta, choice),
            z_delta_claimed: Scalar::conditional_select(
                &a.z_delta_claimed,
                &b.z_delta_claimed,
                choice,
            ),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encryption::pedersen::Pedersen;

    #[test]
    fn test_fee_above_max_proof() {
        let transfer_amount: u64 = 55;
        let max_fee: u64 = 3;

        let rate_fee: u16 = 555; // 5.55%
        let amount_fee: u64 = 4;
        let delta: u64 = 9475; // 4*10000 - 55*555

        let (commitment_transfer, opening_transfer) = Pedersen::new(transfer_amount);
        let (commitment_fee, opening_fee) = Pedersen::new(max_fee);

        let scalar_rate = Scalar::from(rate_fee);
        let commitment_delta =
            &commitment_fee * &Scalar::from(10000_u64) - &commitment_transfer * &scalar_rate;
        let opening_delta =
            &opening_fee * &Scalar::from(10000_u64) - &opening_transfer * &scalar_rate;

        let (commitment_delta_claimed, opening_delta_claimed) = Pedersen::new(0_u64);

        let mut transcript_prover = Transcript::new(b"test");
        let mut transcript_verifier = Transcript::new(b"test");

        let proof = FeeProof::new(
            (amount_fee, &commitment_fee, &opening_fee),
            (delta, &commitment_delta, &opening_delta),
            (&commitment_delta_claimed, &opening_delta_claimed),
            max_fee,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                max_fee,
                &commitment_fee,
                &commitment_delta,
                &commitment_delta_claimed,
                &mut transcript_verifier,
            )
            .is_ok());
    }

    #[test]
    fn test_fee_below_max_proof() {
        let transfer_amount: u64 = 55;
        let max_fee: u64 = 77;

        let rate_fee: u16 = 555; // 5.55%
        let amount_fee: u64 = 4;
        let delta: u64 = 9475; // 4*10000 - 55*555

        let (commitment_transfer, opening_transfer) = Pedersen::new(transfer_amount);
        let (commitment_fee, opening_fee) = Pedersen::new(amount_fee);

        let scalar_rate = Scalar::from(rate_fee);
        let commitment_delta =
            &commitment_fee * &Scalar::from(10000_u64) - &commitment_transfer * &scalar_rate;
        let opening_delta =
            &opening_fee * &Scalar::from(10000_u64) - &opening_transfer * &scalar_rate;

        let (commitment_delta_claimed, opening_delta_claimed) = Pedersen::new(delta);

        assert_eq!(
            commitment_delta.get_point() - opening_delta.get_scalar() * &(*H),
            commitment_delta_claimed.get_point() - opening_delta_claimed.get_scalar() * &(*H)
        );

        let mut transcript_prover = Transcript::new(b"test");
        let mut transcript_verifier = Transcript::new(b"test");

        let proof = FeeProof::new(
            (amount_fee, &commitment_fee, &opening_fee),
            (delta, &commitment_delta, &opening_delta),
            (&commitment_delta_claimed, &opening_delta_claimed),
            max_fee,
            &mut transcript_prover,
        );

        // let proof = FeeProof::create_proof_fee_below_max(
        //     &commitment_fee,
        //     (delta, &opening_delta),
        //     &opening_delta_claimed,
        //     max_fee,
        //     &mut transcript_prover,
        // );

        assert!(proof
            .verify(
                max_fee,
                &commitment_fee,
                &commitment_delta,
                &commitment_delta_claimed,
                &mut transcript_verifier,
            )
            .is_ok());
    }
}
