//! The sigma proofs for transfer fees.
//!
//! TODO: Add detail on how the fee is calculated.

#[cfg(not(target_os = "solana"))]
use {
    crate::encryption::pedersen::{PedersenCommitment, PedersenOpening, G, H},
    rand::rngs::OsRng,
};
use {
    crate::{
        errors::ProofVerificationError, sigma_proofs::errors::FeeSigmaProofError,
        transcript::TranscriptProtocol,
    },
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
#[cfg(not(target_os = "solana"))]
impl FeeSigmaProof {
    /// Creates a fee sigma proof assuming that the committed fee is greater than the maximum fee
    /// bound.
    ///
    /// Note: the proof is generated twice via `create_proof_fee_above_max` and
    /// `create_proof_fee_below_max` to enforce constant time execution.
    ///
    /// * `(fee_amount, fee_commitment, fee_opening)` - The amount, Pedersen commitment, and
    /// opening of the transfer fee
    /// * `(delta_fee, delta_commitment, delta_opening)` - The amount, Pedersen commitment, and
    /// opening of the "real" delta amount
    /// * `(claimed_commitment, claimed_opening)` - The Pedersen commitment and opening of the
    /// "claimed" delta amount
    /// * `max_fee` - The maximum fee bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new(
        (fee_amount, fee_commitment, fee_opening): (u64, &PedersenCommitment, &PedersenOpening),
        (delta_fee, delta_commitment, delta_opening): (u64, &PedersenCommitment, &PedersenOpening),
        (claimed_commitment, claimed_opening): (&PedersenCommitment, &PedersenOpening),
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Self {
        let mut transcript_fee_above_max = transcript.clone();
        let mut transcript_fee_below_max = transcript.clone();

        // compute proof for both cases `fee_amount' >= `max_fee` and `fee_amount` < `max_fee`
        let proof_fee_above_max = Self::create_proof_fee_above_max(
            fee_opening,
            delta_commitment,
            claimed_commitment,
            &mut transcript_fee_above_max,
        );

        let proof_fee_below_max = Self::create_proof_fee_below_max(
            fee_commitment,
            (delta_fee, delta_opening),
            claimed_opening,
            max_fee,
            &mut transcript_fee_below_max,
        );

        let below_max = u64::ct_gt(&max_fee, &fee_amount);

        // choose one of `proof_fee_above_max` or `proof_fee_below_max` according to whether the
        // fee amount surpasses max fee
        let fee_max_proof = FeeMaxProof::conditional_select(
            &proof_fee_above_max.fee_max_proof,
            &proof_fee_below_max.fee_max_proof,
            below_max,
        );

        let fee_equality_proof = FeeEqualityProof::conditional_select(
            &proof_fee_above_max.fee_equality_proof,
            &proof_fee_below_max.fee_equality_proof,
            below_max,
        );

        transcript.append_point(b"Y_max_proof", &fee_max_proof.Y_max_proof);
        transcript.append_point(b"Y_delta", &fee_equality_proof.Y_delta);
        transcript.append_point(b"Y_claimed", &fee_equality_proof.Y_claimed);
        transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        Self {
            fee_max_proof,
            fee_equality_proof,
        }
    }

    /// Creates a fee sigma proof assuming that the committed fee is greater than the maximum fee
    /// bound.
    ///
    /// * `fee_opening` - The opening of the Pedersen commitment of the transfer fee
    /// * `delta_commitment` - The Pedersen commitment of the "real" delta value
    /// * `claimed_commitment` - The Pedersen commitment of the "claimed" delta value
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    fn create_proof_fee_above_max(
        fee_opening: &PedersenOpening,
        delta_commitment: &PedersenCommitment,
        claimed_commitment: &PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Self {
        // simulate equality proof
        let C_delta = delta_commitment.get_point();
        let C_claimed = claimed_commitment.get_point();

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
        let r_fee = fee_opening.get_scalar();

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
    /// * `fee_commitment` - The Pedersen commitment of the transfer fee
    /// * `(delta_fee, delta_opening)` - The Pedersen commitment and opening of the "real" delta
    /// value
    /// * `claimed_opening` - The opening of the Pedersen commitment of the "claimed" delta value
    /// * `max_fee` - The maximum fee bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    fn create_proof_fee_below_max(
        fee_commitment: &PedersenCommitment,
        (delta_fee, delta_opening): (u64, &PedersenOpening),
        claimed_opening: &PedersenOpening,
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Self {
        // simulate max proof
        let m = Scalar::from(max_fee);
        let C_fee = fee_commitment.get_point();

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

        let r_delta = delta_opening.get_scalar();
        let r_claimed = claimed_opening.get_scalar();

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
    /// * `fee_commitment` - The Pedersen commitment of the transfer fee
    /// * `delta_commitment` - The Pedersen commitment of the "real" delta value
    /// * `claimed_commitment` - The Pedersen commitment of the "claimed" delta value
    /// * `max_fee` - The maximum fee bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        fee_commitment: &PedersenCommitment,
        delta_commitment: &PedersenCommitment,
        claimed_commitment: &PedersenCommitment,
        max_fee: u64,
        transcript: &mut Transcript,
    ) -> Result<(), FeeSigmaProofError> {
        // extract the relevant scalar and Ristretto points from the input
        let m = Scalar::from(max_fee);

        let C_max = fee_commitment.get_point();
        let C_delta = delta_commitment.get_point();
        let C_claimed = claimed_commitment.get_point();

        transcript.validate_and_append_point(b"Y_max_proof", &self.fee_max_proof.Y_max_proof)?;
        transcript.validate_and_append_point(b"Y_delta", &self.fee_equality_proof.Y_delta)?;
        transcript.validate_and_append_point(b"Y_claimed", &self.fee_equality_proof.Y_claimed)?;

        let Y_max = self
            .fee_max_proof
            .Y_max_proof
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;
        let z_max = self.fee_max_proof.z_max_proof;

        let Y_delta_real = self
            .fee_equality_proof
            .Y_delta
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;
        let Y_claimed = self
            .fee_equality_proof
            .Y_claimed
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;
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
            Err(ProofVerificationError::AlgebraicRelation.into())
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
        if bytes.len() != 256 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let bytes = array_ref![bytes, 0, 256];
        let (Y_max_proof, z_max_proof, c_max_proof, Y_delta, Y_claimed, z_x, z_delta, z_claimed) =
            array_refs![bytes, 32, 32, 32, 32, 32, 32, 32, 32];

        let Y_max_proof = CompressedRistretto::from_slice(Y_max_proof);
        let z_max_proof = Scalar::from_canonical_bytes(*z_max_proof)
            .ok_or(ProofVerificationError::Deserialization)?;
        let c_max_proof = Scalar::from_canonical_bytes(*c_max_proof)
            .ok_or(ProofVerificationError::Deserialization)?;

        let Y_delta = CompressedRistretto::from_slice(Y_delta);
        let Y_claimed = CompressedRistretto::from_slice(Y_claimed);
        let z_x =
            Scalar::from_canonical_bytes(*z_x).ok_or(ProofVerificationError::Deserialization)?;
        let z_delta = Scalar::from_canonical_bytes(*z_delta)
            .ok_or(ProofVerificationError::Deserialization)?;
        let z_claimed = Scalar::from_canonical_bytes(*z_claimed)
            .ok_or(ProofVerificationError::Deserialization)?;

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

        let fee_rate: u16 = 555; // 5.55%
        let fee_amount: u64 = 4;
        let delta: u64 = 9475; // 4*10000 - 55*555

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (fee_commitment, fee_opening) = Pedersen::new(max_fee);

        let scalar_rate = Scalar::from(fee_rate);
        let delta_commitment =
            &fee_commitment * &Scalar::from(10000_u64) - &transfer_commitment * &scalar_rate;
        let delta_opening =
            &fee_opening * &Scalar::from(10000_u64) - &transfer_opening * &scalar_rate;

        let (claimed_commitment, claimed_opening) = Pedersen::new(0_u64);

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = FeeSigmaProof::new(
            (fee_amount, &fee_commitment, &fee_opening),
            (delta, &delta_commitment, &delta_opening),
            (&claimed_commitment, &claimed_opening),
            max_fee,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                &fee_commitment,
                &delta_commitment,
                &claimed_commitment,
                max_fee,
                &mut verifier_transcript,
            )
            .is_ok());
    }

    #[test]
    fn test_fee_below_max_proof() {
        let transfer_amount: u64 = 1;
        let max_fee: u64 = 3;

        let fee_rate: u16 = 400; // 5.55%
        let fee_amount: u64 = 1;
        let delta: u64 = 9600; // 4*10000 - 55*555

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (fee_commitment, fee_opening) = Pedersen::new(fee_amount);

        let scalar_rate = Scalar::from(fee_rate);
        let delta_commitment =
            &fee_commitment * &Scalar::from(10000_u64) - &transfer_commitment * &scalar_rate;
        let delta_opening =
            &fee_opening * &Scalar::from(10000_u64) - &transfer_opening * &scalar_rate;

        let (claimed_commitment, claimed_opening) = Pedersen::new(delta);

        assert_eq!(
            delta_commitment.get_point() - delta_opening.get_scalar() * &(*H),
            claimed_commitment.get_point() - claimed_opening.get_scalar() * &(*H)
        );

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = FeeSigmaProof::new(
            (fee_amount, &fee_commitment, &fee_opening),
            (delta, &delta_commitment, &delta_opening),
            (&claimed_commitment, &claimed_opening),
            max_fee,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                &fee_commitment,
                &delta_commitment,
                &claimed_commitment,
                max_fee,
                &mut verifier_transcript,
            )
            .is_ok());
    }

    #[test]
    fn test_fee_delta_is_zero() {
        let transfer_amount: u64 = 100;
        let max_fee: u64 = 3;

        let fee_rate: u16 = 100; // 1.00%
        let fee_amount: u64 = 1;
        let delta: u64 = 0; // 1*10000 - 100*100

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (fee_commitment, fee_opening) = Pedersen::new(fee_amount);

        let scalar_rate = Scalar::from(fee_rate);
        let delta_commitment =
            &(&fee_commitment * &Scalar::from(10000_u64)) - &(&transfer_commitment * &scalar_rate);
        let delta_opening =
            &(&fee_opening * &Scalar::from(10000_u64)) - &(&transfer_opening * &scalar_rate);

        let (claimed_commitment, claimed_opening) = Pedersen::new(delta);

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = FeeSigmaProof::new(
            (fee_amount, &fee_commitment, &fee_opening),
            (delta, &delta_commitment, &delta_opening),
            (&claimed_commitment, &claimed_opening),
            max_fee,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                &fee_commitment,
                &delta_commitment,
                &claimed_commitment,
                max_fee,
                &mut verifier_transcript,
            )
            .is_ok());
    }
}
