//! The sigma proofs for transfer fees.
//!
//! TODO: Add detail on how the fee is calculated.

#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::pedersen::{PedersenCommitment, PedersenOpening, G, H},
    rand::rngs::OsRng,
};
use {
    crate::{sigma_proofs::errors::FeeSigmaProofError, transcript::TranscriptProtocol},
    arrayref::{array_ref, array_refs},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, MultiscalarMul, VartimeMultiscalarMul},
    },
    merlin::Transcript,
    subtle::{Choice, ConditionallySelectable, ConstantTimeGreater},
};

/// Fee sigma proof.
///
/// The proof consists of two main components: `fee_max_proof` and `fee_equality_proof`. If the fee
/// surpasses the maximum fee bound, then the `fee_max_proof` should properly be generated and
/// `fee_equality_proof` should be simulated. If the fee is below the maximum fee bound, then the
/// `fee_equality_proof` should be properly generated and `fee_max_proof` should be simulated.
#[derive(Clone)]
pub struct FeeSigmaProof {
    /// Proof that the committed fee amount equals the maximum fee bound
    fee_max_proof: FeeMaxProof,

    /// Proof that the "real" delta value is equal to the "claimed" delta value
    fee_equality_proof: FeeEqualityProof,
}

#[allow(non_snake_case, dead_code)]
#[cfg(not(target_arch = "bpf"))]
impl FeeSigmaProof {
    /// Creates a fee sigma proof assuming that the committed fee is greater than the maximum fee
    /// bound.
    ///
    /// * `(fee_amount, commitment_fee, opening_fee)` - The amount, Pedersen commitment, and
    /// opening of the transfer fee
    /// * `(delta_fee, commitment_delta, opening_delta)` - The amount, Pedersen commitment, and
    /// opening of the "real" delta amount
    /// * `(commitment_claimed, opening_claimed)` - The Pedersen commitment and opening of the
    /// "claimed" delta amount
    /// * `max_fee` - The maximum fee bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new(
        (fee_amount, commitment_fee, opening_fee): (u64, &PedersenCommitment, &PedersenOpening),
        (delta_fee, commitment_delta, opening_delta): (u64, &PedersenCommitment, &PedersenOpening),
        (commitment_claimed, opening_claimed): (&PedersenCommitment, &PedersenOpening),
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Self {
        let mut transcript_fee_above_max = transcript.clone();
        let mut transcript_fee_below_max = transcript.clone();

        // compute proof for both cases `fee_amount' >= `max_fee` and `fee_amount` < `max_fee`
        let proof_fee_above_max = Self::create_proof_fee_above_max(
            opening_fee,
            commitment_delta,
            commitment_claimed,
            &mut transcript_fee_above_max,
        );

        let proof_fee_below_max = Self::create_proof_fee_below_max(
            commitment_fee,
            (delta_fee, opening_delta),
            opening_claimed,
            max_fee,
            &mut transcript_fee_below_max,
        );

        let below_max = u64::ct_gt(&max_fee, &fee_amount);

        // conditionally assign transcript; transcript is not conditionally selectable
        if bool::from(below_max) {
            *transcript = transcript_fee_below_max;
        } else {
            *transcript = transcript_fee_above_max;
        }

        Self {
            fee_max_proof: FeeMaxProof::conditional_select(
                &proof_fee_above_max.fee_max_proof,
                &proof_fee_below_max.fee_max_proof,
                below_max,
            ),
            fee_equality_proof: FeeEqualityProof::conditional_select(
                &proof_fee_above_max.fee_equality_proof,
                &proof_fee_below_max.fee_equality_proof,
                below_max,
            ),
        }
    }

    /// Creates a fee sigma proof assuming that the committed fee is greater than the maximum fee
    /// bound.
    ///
    /// * `opening_fee` - The opening of the Pedersen commitment of the transfer fee
    /// * `commitment_delta` - The Pedersen commitment of the "real" delta value
    /// * `commitment_claimed` - The Pedersen commitment of the "claimed" delta value
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    fn create_proof_fee_above_max(
        opening_fee: &PedersenOpening,
        commitment_delta: &PedersenCommitment,
        commitment_claimed: &PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Self {
        // simulate equality proof
        let C_delta = commitment_delta.get_point();
        let C_claimed = commitment_claimed.get_point();

        let z_x = Scalar::random(&mut OsRng);
        let z_delta = Scalar::random(&mut OsRng);
        let z_claimed = Scalar::random(&mut OsRng);
        let c_equality = Scalar::random(&mut OsRng);

        let Y_delta = RistrettoPoint::multiscalar_mul(
            vec![z_x, z_delta, -c_equality],
            vec![&(*G), &(*H), C_delta],
        )
        .compress();

        let Y_claimed = RistrettoPoint::multiscalar_mul(
            vec![z_x, z_claimed, -c_equality],
            vec![&(*G), &(*H), C_claimed],
        )
        .compress();

        let fee_equality_proof = FeeEqualityProof {
            Y_delta,
            Y_claimed,
            z_x,
            z_delta,
            z_claimed,
        };

        // generate max proof
        let r_fee = opening_fee.get_scalar();

        let y_max_proof = Scalar::random(&mut OsRng);
        let Y_max_proof = (y_max_proof * &(*H)).compress();

        transcript.append_point(b"Y_max_proof", &Y_max_proof);
        transcript.append_point(b"Y_delta", &Y_delta);
        transcript.append_point(b"Y_claimed", &Y_claimed);

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

    /// Creates a fee sigma proof assuming that the committed fee is less than the maximum fee
    /// bound.
    ///
    /// * `commitment_fee` - The Pedersen commitment of the transfer fee
    /// * `(delta_fee, opening_delta)` - The Pedersen commitment and opening of the "real" delta
    /// value
    /// * `opening_claimed` - The opening of the Pedersen commitment of the "claimed" delta value
    /// * `max_fee` - The maximum fee bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    fn create_proof_fee_below_max(
        commitment_fee: &PedersenCommitment,
        (delta_fee, opening_delta): (u64, &PedersenOpening),
        opening_claimed: &PedersenOpening,
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
        let r_claimed = opening_claimed.get_scalar();

        let y_x = Scalar::random(&mut OsRng);
        let y_delta = Scalar::random(&mut OsRng);
        let y_claimed = Scalar::random(&mut OsRng);

        let Y_delta =
            RistrettoPoint::multiscalar_mul(vec![y_x, y_delta], vec![&(*G), &(*H)]).compress();
        let Y_claimed =
            RistrettoPoint::multiscalar_mul(vec![y_x, y_claimed], vec![&(*G), &(*H)]).compress();

        transcript.append_point(b"Y_max_proof", &Y_max_proof);
        transcript.append_point(b"Y_delta", &Y_delta);
        transcript.append_point(b"Y_claimed", &Y_claimed);

        let c = transcript.challenge_scalar(b"c");
        let c_equality = c - c_max_proof;

        transcript.challenge_scalar(b"w");

        let z_x = c_equality * x + y_x;
        let z_delta = c_equality * r_delta + y_delta;
        let z_claimed = c_equality * r_claimed + y_claimed;

        let fee_equality_proof = FeeEqualityProof {
            Y_delta,
            Y_claimed,
            z_x,
            z_delta,
            z_claimed,
        };

        Self {
            fee_max_proof,
            fee_equality_proof,
        }
    }

    /// Fee sigma proof verifier.
    ///
    /// * `commitment_fee` - The Pedersen commitment of the transfer fee
    /// * `commitment_delta` - The Pedersen commitment of the "real" delta value
    /// * `commitment_claimed` - The Pedersen commitment of the "claimed" delta value
    /// * `max_fee` - The maximum fee bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        commitment_fee: &PedersenCommitment,
        commitment_delta: &PedersenCommitment,
        commitment_claimed: &PedersenCommitment,
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Result<(), FeeSigmaProofError> {
        // extract the relevant scalar and Ristretto points from the input
        let m = Scalar::from(max_fee);

        let C_max = commitment_fee.get_point();
        let C_delta = commitment_delta.get_point();
        let C_claimed = commitment_claimed.get_point();

        transcript.validate_and_append_point(b"Y_max_proof", &self.fee_max_proof.Y_max_proof)?;
        transcript.validate_and_append_point(b"Y_delta", &self.fee_equality_proof.Y_delta)?;
        transcript.validate_and_append_point(b"Y_claimed", &self.fee_equality_proof.Y_claimed)?;

        let Y_max = self
            .fee_max_proof
            .Y_max_proof
            .decompress()
            .ok_or(FeeSigmaProofError::Format)?;
        let z_max = self.fee_max_proof.z_max_proof;

        let Y_delta_real = self
            .fee_equality_proof
            .Y_delta
            .decompress()
            .ok_or(FeeSigmaProofError::Format)?;
        let Y_claimed = self
            .fee_equality_proof
            .Y_claimed
            .decompress()
            .ok_or(FeeSigmaProofError::Format)?;
        let z_x = self.fee_equality_proof.z_x;
        let z_delta_real = self.fee_equality_proof.z_delta;
        let z_claimed = self.fee_equality_proof.z_claimed;

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
                ww * z_claimed,
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
                C_claimed,
                &Y_claimed,
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(FeeSigmaProofError::AlgebraicRelation)
        }
    }

    pub fn to_bytes(&self) -> [u8; 256] {
        let mut buf = [0_u8; 256];
        buf[..32].copy_from_slice(self.fee_max_proof.Y_max_proof.as_bytes());
        buf[32..64].copy_from_slice(self.fee_max_proof.z_max_proof.as_bytes());
        buf[64..96].copy_from_slice(self.fee_max_proof.c_max_proof.as_bytes());
        buf[96..128].copy_from_slice(self.fee_equality_proof.Y_delta.as_bytes());
        buf[128..160].copy_from_slice(self.fee_equality_proof.Y_claimed.as_bytes());
        buf[160..192].copy_from_slice(self.fee_equality_proof.z_x.as_bytes());
        buf[192..224].copy_from_slice(self.fee_equality_proof.z_delta.as_bytes());
        buf[224..256].copy_from_slice(self.fee_equality_proof.z_claimed.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FeeSigmaProofError> {
        let bytes = array_ref![bytes, 0, 256];
        let (Y_max_proof, z_max_proof, c_max_proof, Y_delta, Y_claimed, z_x, z_delta, z_claimed) =
            array_refs![bytes, 32, 32, 32, 32, 32, 32, 32, 32];

        let Y_max_proof = CompressedRistretto::from_slice(Y_max_proof);
        let z_max_proof =
            Scalar::from_canonical_bytes(*z_max_proof).ok_or(FeeSigmaProofError::Format)?;
        let c_max_proof =
            Scalar::from_canonical_bytes(*c_max_proof).ok_or(FeeSigmaProofError::Format)?;

        let Y_delta = CompressedRistretto::from_slice(Y_delta);
        let Y_claimed = CompressedRistretto::from_slice(Y_claimed);
        let z_x = Scalar::from_canonical_bytes(*z_x).ok_or(FeeSigmaProofError::Format)?;
        let z_delta = Scalar::from_canonical_bytes(*z_delta).ok_or(FeeSigmaProofError::Format)?;
        let z_claimed =
            Scalar::from_canonical_bytes(*z_claimed).ok_or(FeeSigmaProofError::Format)?;

        Ok(Self {
            fee_max_proof: FeeMaxProof {
                Y_max_proof,
                z_max_proof,
                c_max_proof,
            },
            fee_equality_proof: FeeEqualityProof {
                Y_delta,
                Y_claimed,
                z_x,
                z_delta,
                z_claimed,
            },
        })
    }
}

/// The fee max proof.
///
/// The proof certifies that the transfer fee Pedersen commitment encodes the maximum fee bound.
#[allow(non_snake_case)]
#[derive(Clone, Copy)]
pub struct FeeMaxProof {
    Y_max_proof: CompressedRistretto,
    z_max_proof: Scalar,
    c_max_proof: Scalar,
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

/// The fee equality proof.
///
/// The proof certifies that the "real" delta value commitment and the "claimed" delta value
/// commitment encode the same message.
#[allow(non_snake_case)]
#[derive(Clone, Copy)]
pub struct FeeEqualityProof {
    Y_delta: CompressedRistretto,
    Y_claimed: CompressedRistretto,
    z_x: Scalar,
    z_delta: Scalar,
    z_claimed: Scalar,
}

impl ConditionallySelectable for FeeEqualityProof {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        Self {
            Y_delta: conditional_select_ristretto(&a.Y_delta, &b.Y_delta, choice),
            Y_claimed: conditional_select_ristretto(&a.Y_claimed, &b.Y_claimed, choice),
            z_x: Scalar::conditional_select(&a.z_x, &b.z_x, choice),
            z_delta: Scalar::conditional_select(&a.z_delta, &b.z_delta, choice),
            z_claimed: Scalar::conditional_select(&a.z_claimed, &b.z_claimed, choice),
        }
    }
}

#[allow(clippy::needless_range_loop)]
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

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::pedersen::Pedersen};

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

        let (commitment_claimed, opening_claimed) = Pedersen::new(0_u64);

        let mut transcript_prover = Transcript::new(b"test");
        let mut transcript_verifier = Transcript::new(b"test");

        let proof = FeeSigmaProof::new(
            (amount_fee, &commitment_fee, &opening_fee),
            (delta, &commitment_delta, &opening_delta),
            (&commitment_claimed, &opening_claimed),
            max_fee,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment_fee,
                &commitment_delta,
                &commitment_claimed,
                max_fee,
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

        let (commitment_claimed, opening_claimed) = Pedersen::new(delta);

        assert_eq!(
            commitment_delta.get_point() - opening_delta.get_scalar() * &(*H),
            commitment_claimed.get_point() - opening_claimed.get_scalar() * &(*H)
        );

        let mut transcript_prover = Transcript::new(b"test");
        let mut transcript_verifier = Transcript::new(b"test");

        let proof = FeeSigmaProof::new(
            (amount_fee, &commitment_fee, &opening_fee),
            (delta, &commitment_delta, &opening_delta),
            (&commitment_claimed, &opening_claimed),
            max_fee,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment_fee,
                &commitment_delta,
                &commitment_claimed,
                max_fee,
                &mut transcript_verifier,
            )
            .is_ok());
    }

    #[test]
    fn test_fee_delta_is_zero() {
        let transfer_amount: u64 = 100;
        let max_fee: u64 = 3;

        let rate_fee: u16 = 100; // 1.00%
        let amount_fee: u64 = 1;
        let delta: u64 = 0; // 1*10000 - 100*100

        let (commitment_transfer, opening_transfer) = Pedersen::new(transfer_amount);
        let (commitment_fee, opening_fee) = Pedersen::new(amount_fee);

        let scalar_rate = Scalar::from(rate_fee);
        let commitment_delta =
            &(&commitment_fee * &Scalar::from(10000_u64)) - &(&commitment_transfer * &scalar_rate);
        let opening_delta =
            &(&opening_fee * &Scalar::from(10000_u64)) - &(&opening_transfer * &scalar_rate);

        let (commitment_claimed, opening_claimed) = Pedersen::new(delta);

        let mut transcript_prover = Transcript::new(b"test");
        let mut transcript_verifier = Transcript::new(b"test");

        let proof = FeeSigmaProof::new(
            (amount_fee, &commitment_fee, &opening_fee),
            (delta, &commitment_delta, &opening_delta),
            (&commitment_claimed, &opening_claimed),
            max_fee,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment_fee,
                &commitment_delta,
                &commitment_claimed,
                max_fee,
                &mut transcript_verifier,
            )
            .is_ok());
    }
}
