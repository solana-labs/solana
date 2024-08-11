//! The percentage-with-cap sigma proof.
//!
//! The proof is defined with respect to three Pedersen commitments that encodes values referred to
//! as a `percentage`, `delta`, and `claimed` amounts. The proof certifies that either
//! - the `percentage` amount is equal to a constant (referred to as the `max_value`)
//! - the `delta` and `claimed` amounts are equal
//!
//! This type of proof is useful as a building block to prove that, given two Pedersen commitments,
//! one encodes a percentage value of the number encoded by the other commitment with a specified
//! max cap value.
//!
//! A more detailed description of the context and how the proof is computed is provided in the
//! [`ZK Token proof program`] documentation.
//!
//! The protocol guarantees computational soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.
//!
//! [`ZK Token proof program`]: https://docs.solanalabs.com/runtime/zk-token-proof

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::pedersen::{PedersenCommitment, PedersenOpening, G, H},
        sigma_proofs::{canonical_scalar_from_optional_slice, ristretto_point_from_optional_slice},
        UNIT_LEN,
    },
    rand::rngs::OsRng,
};
use {
    crate::{
        sigma_proofs::errors::{
            PercentageWithCapProofVerificationError, SigmaProofVerificationError,
        },
        transcript::TranscriptProtocol,
    },
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, MultiscalarMul, VartimeMultiscalarMul},
    },
    merlin::Transcript,
    subtle::{Choice, ConditionallySelectable, ConstantTimeGreater},
};

/// Byte length of a percentage-with-cap proof.
const PERCENTAGE_WITH_CAP_PROOF_LEN: usize = UNIT_LEN * 8;

/// Percentage-with-cap proof.
///
/// The proof consists of two main components: `percentage_max_proof` and
/// `percentage_equality_proof`. If the committed amount is greater than the maximum cap value,
/// then the `percentage_max_proof` is properly generated and `percentage_equality_proof` is
/// simulated. If the encrypted amount is smaller than the maximum cap bound, the
/// `percentage_equality_proof` is properly generated and `percentage_max_proof` is simulated.
#[derive(Clone)]
pub struct PercentageWithCapProof {
    /// Proof that the committed amount equals the maximum cap bound
    percentage_max_proof: PercentageMaxProof,

    /// Proof that the `delta` value is equal to the `claimed` value
    percentage_equality_proof: PercentageEqualityProof,
}

#[allow(non_snake_case, dead_code)]
#[cfg(not(target_os = "solana"))]
impl PercentageWithCapProof {
    /// Creates a percentage-with-cap sigma proof.
    ///
    /// A typical percentage-with-cap application is defined with respect to the following values:
    /// - a commitment encoding a `base_amount` and a commitment encoding a `percentage_amount`
    /// - two constants `percentage_rate_basis_point`, which defines the percentage rate in units
    ///   of 0.01% and `max_value`, which defines the max cap amount.
    ///
    /// This setting requires that the `percentage_amount` is either a certain percentage of the
    /// `base_amount` (determined by the `percentage_rate_basis_point`) or is equal to the max cap
    /// amount (determined by `max_value`).
    ///
    /// If `percentage_amount < max_value`, then assuming that there is no division rounding, the
    /// `percentage_amount` must satisfy the relation `transfer_amount *
    /// (percentage_rate_basis_point / 10_000) = percentage_amount` or equivalently, `(base_amount
    /// * percentage_rate_basis_point) - (10_000 * percentage_amount) = 0`. More generally, let
    /// `delta_amount = (base_amount * percentage_rate_basis_point) - (10_000 * percentage_amount)`.
    /// Then assuming that a division rounding could occur, the `delta_amount` must satisfy the
    /// bound `0 <= delta_amount < 10_000`.
    ///
    /// If `percentage_amount >= max_amount`, then `percentage_amount = max_value` and therefore,
    /// the prover can generate a proof certifying that a percentage commitment exactly encodes
    /// `max_value`. If `percentage_amount < max_value`, then the prover can create a commitment
    /// (referred to as the `claimed_amount`) to `delta_amount` and create a range proof certifying
    /// that the committed value satisfies the bound `0 <= delta_amount < 10_000`.
    ///
    /// Since the type of proof that a prover generates reveals information about the base and
    /// percentage amounts, the prover must generate and include both types of proofs. If
    /// `percentage_amount >= max_value`, then the prover generates a valid `percentage_max_proof`,
    /// but commits to 0 as the `claimed_amount` and simulates ("fakes") a proof
    /// (`percentage_equality_proof`) that this is valid. If `percentage_amount > max_value`, then
    /// the prover simulates a `percentage_max_proof`, and creates a valid
    /// `percentage_equality_proof` certifying that the claimed delta value is equal to the "real"
    /// delta value.
    ///
    /// Note: In the implementation, the proof is generated twice via `create_proof_above_max`
    /// and `create_proof_below_max` to enforce that the function executes in constant time.
    ///
    /// * `percentage_commitment` - The Pedersen commitment to a percentage amount
    /// * `percentage_opening` - The Pedersen opening of a percentage amount
    /// * `percentage_amount` - The percentage amount
    /// * `delta_commitment` - The Pedersen commitment to a delta amount
    /// * `delta_opening` - The Pedersen opening of a delta amount
    /// * `delta_amount` - The delta amount
    /// * `claimed_commitment` - The Pedersen commitment to a claimed amount
    /// * `claimed_opening` - The Pedersen opening of a claimed amount
    /// * `max_value` - The maximum cap bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        percentage_commitment: &PedersenCommitment,
        percentage_opening: &PedersenOpening,
        percentage_amount: u64,
        delta_commitment: &PedersenCommitment,
        delta_opening: &PedersenOpening,
        delta_amount: u64,
        claimed_commitment: &PedersenCommitment,
        claimed_opening: &PedersenOpening,
        max_value: u64,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.percentage_with_cap_proof_domain_separator();
        let mut transcript_percentage_above_max = transcript.clone();
        let mut transcript_percentage_below_max = transcript.clone();

        // compute proof for both cases `percentage_amount' >= `max_value` and
        // `percentage_amount` < `max_value`
        let proof_above_max = Self::create_proof_percentage_above_max(
            percentage_opening,
            delta_commitment,
            claimed_commitment,
            &mut transcript_percentage_above_max,
        );

        let proof_below_max = Self::create_proof_percentage_below_max(
            percentage_commitment,
            delta_opening,
            delta_amount,
            claimed_opening,
            max_value,
            &mut transcript_percentage_below_max,
        );

        let below_max = u64::ct_gt(&max_value, &percentage_amount);

        // choose one of `proof_above_max` or `proof_below_max` according to whether the
        // percentage amount is greater than `max_value` or not
        let percentage_max_proof = PercentageMaxProof::conditional_select(
            &proof_above_max.percentage_max_proof,
            &proof_below_max.percentage_max_proof,
            below_max,
        );

        let percentage_equality_proof = PercentageEqualityProof::conditional_select(
            &proof_above_max.percentage_equality_proof,
            &proof_below_max.percentage_equality_proof,
            below_max,
        );

        transcript.append_point(b"Y_max_proof", &percentage_max_proof.Y_max_proof);
        transcript.append_point(b"Y_delta", &percentage_equality_proof.Y_delta);
        transcript.append_point(b"Y_claimed", &percentage_equality_proof.Y_claimed);
        transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        Self {
            percentage_max_proof,
            percentage_equality_proof,
        }
    }

    /// Creates a percentage-with-cap proof assuming that the committed percentage is greater than
    /// the maximum cap bound.
    ///
    /// * `percentage_opening` - The Pedersen opening of a percentage amount
    /// * `delta_commitment` - The Pedersen commitment to a delta amount
    /// * `claimed_commitment` - The Pedersen commitment to a claimed amount
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    fn create_proof_percentage_above_max(
        percentage_opening: &PedersenOpening,
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

        let percentage_equality_proof = PercentageEqualityProof {
            Y_delta,
            Y_claimed,
            z_x,
            z_delta,
            z_claimed,
        };

        // generate max proof
        let r_percentage = percentage_opening.get_scalar();

        let y_max_proof = Scalar::random(&mut OsRng);
        let Y_max_proof = (y_max_proof * &(*H)).compress();

        transcript.append_point(b"Y_max_proof", &Y_max_proof);
        transcript.append_point(b"Y_delta", &Y_delta);
        transcript.append_point(b"Y_claimed", &Y_claimed);

        let c = transcript.challenge_scalar(b"c");
        let c_max_proof = c - c_equality;

        transcript.challenge_scalar(b"w");

        let z_max_proof = c_max_proof * r_percentage + y_max_proof;

        let percentage_max_proof = PercentageMaxProof {
            Y_max_proof,
            z_max_proof,
            c_max_proof,
        };

        Self {
            percentage_max_proof,
            percentage_equality_proof,
        }
    }

    /// Creates a percentage-with-cap proof assuming that the committed amount is less than the
    /// maximum cap bound.
    ///
    /// * `percentage_commitment` - The Pedersen commitment to a percentage amount
    /// * `delta_opening` - The Pedersen opening of a delta amount
    /// * `delta_amount` - The delta amount
    /// * `max_value` - The maximum cap bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    fn create_proof_percentage_below_max(
        percentage_commitment: &PedersenCommitment,
        delta_opening: &PedersenOpening,
        delta_amount: u64,
        claimed_opening: &PedersenOpening,
        max_value: u64,
        transcript: &mut Transcript,
    ) -> Self {
        // simulate max proof
        let m = Scalar::from(max_value);
        let C_percentage = percentage_commitment.get_point();

        let z_max_proof = Scalar::random(&mut OsRng);
        let c_max_proof = Scalar::random(&mut OsRng); // random challenge

        // solve for Y_max in the verification algebraic relation
        let Y_max_proof = RistrettoPoint::multiscalar_mul(
            vec![z_max_proof, -c_max_proof, c_max_proof * m],
            vec![&(*H), C_percentage, &(*G)],
        )
        .compress();

        let percentage_max_proof = PercentageMaxProof {
            Y_max_proof,
            z_max_proof,
            c_max_proof,
        };

        // generate equality proof
        let x = Scalar::from(delta_amount);

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

        let percentage_equality_proof = PercentageEqualityProof {
            Y_delta,
            Y_claimed,
            z_x,
            z_delta,
            z_claimed,
        };

        Self {
            percentage_max_proof,
            percentage_equality_proof,
        }
    }

    /// Verifies a percentage-with-cap proof.
    ///
    /// * `percentage_commitment` - The Pedersen commitment of the value being proved
    /// * `delta_commitment` - The Pedersen commitment of the "real" delta value
    /// * `claimed_commitment` - The Pedersen commitment of the "claimed" delta value
    /// * `max_value` - The maximum cap bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    ///
    /// * `percentage_commitment` - The Pedersen commitment to a percentage amount
    /// * `delta_commitment` - The Pedersen commitment to a delta amount
    /// * `claimed_commitment` - The Pedersen commitment to a claimed amount
    /// * `max_value` - The maximum cap bound
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        percentage_commitment: &PedersenCommitment,
        delta_commitment: &PedersenCommitment,
        claimed_commitment: &PedersenCommitment,
        max_value: u64,
        transcript: &mut Transcript,
    ) -> Result<(), PercentageWithCapProofVerificationError> {
        transcript.percentage_with_cap_proof_domain_separator();

        // extract the relevant scalar and Ristretto points from the input
        let m = Scalar::from(max_value);

        let C_max = percentage_commitment.get_point();
        let C_delta = delta_commitment.get_point();
        let C_claimed = claimed_commitment.get_point();

        transcript
            .validate_and_append_point(b"Y_max_proof", &self.percentage_max_proof.Y_max_proof)?;
        transcript
            .validate_and_append_point(b"Y_delta", &self.percentage_equality_proof.Y_delta)?;
        transcript
            .validate_and_append_point(b"Y_claimed", &self.percentage_equality_proof.Y_claimed)?;

        let Y_max = self
            .percentage_max_proof
            .Y_max_proof
            .decompress()
            .ok_or(SigmaProofVerificationError::Deserialization)?;
        let z_max = self.percentage_max_proof.z_max_proof;

        let Y_delta_real = self
            .percentage_equality_proof
            .Y_delta
            .decompress()
            .ok_or(SigmaProofVerificationError::Deserialization)?;
        let Y_claimed = self
            .percentage_equality_proof
            .Y_claimed
            .decompress()
            .ok_or(SigmaProofVerificationError::Deserialization)?;
        let z_x = self.percentage_equality_proof.z_x;
        let z_delta_real = self.percentage_equality_proof.z_delta;
        let z_claimed = self.percentage_equality_proof.z_claimed;

        let c = transcript.challenge_scalar(b"c");
        let c_max_proof = self.percentage_max_proof.c_max_proof;
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
            Err(SigmaProofVerificationError::AlgebraicRelation.into())
        }
    }

    pub fn to_bytes(&self) -> [u8; PERCENTAGE_WITH_CAP_PROOF_LEN] {
        let mut buf = [0_u8; PERCENTAGE_WITH_CAP_PROOF_LEN];
        let mut chunks = buf.chunks_mut(UNIT_LEN);
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_max_proof.Y_max_proof.as_bytes());
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_max_proof.z_max_proof.as_bytes());
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_max_proof.c_max_proof.as_bytes());
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_equality_proof.Y_delta.as_bytes());
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_equality_proof.Y_claimed.as_bytes());
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_equality_proof.z_x.as_bytes());
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_equality_proof.z_delta.as_bytes());
        chunks
            .next()
            .unwrap()
            .copy_from_slice(self.percentage_equality_proof.z_claimed.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PercentageWithCapProofVerificationError> {
        let mut chunks = bytes.chunks(UNIT_LEN);
        let Y_max_proof = ristretto_point_from_optional_slice(chunks.next())?;
        let z_max_proof = canonical_scalar_from_optional_slice(chunks.next())?;
        let c_max_proof = canonical_scalar_from_optional_slice(chunks.next())?;

        let Y_delta = ristretto_point_from_optional_slice(chunks.next())?;
        let Y_claimed = ristretto_point_from_optional_slice(chunks.next())?;
        let z_x = canonical_scalar_from_optional_slice(chunks.next())?;
        let z_delta = canonical_scalar_from_optional_slice(chunks.next())?;
        let z_claimed = canonical_scalar_from_optional_slice(chunks.next())?;

        Ok(Self {
            percentage_max_proof: PercentageMaxProof {
                Y_max_proof,
                z_max_proof,
                c_max_proof,
            },
            percentage_equality_proof: PercentageEqualityProof {
                Y_delta,
                Y_claimed,
                z_x,
                z_delta,
                z_claimed,
            },
        })
    }
}

/// The percentage max proof.
///
/// The proof certifies that a Pedersen commitment encodes the maximum cap bound.
#[allow(non_snake_case)]
#[derive(Clone, Copy)]
pub struct PercentageMaxProof {
    Y_max_proof: CompressedRistretto,
    z_max_proof: Scalar,
    c_max_proof: Scalar,
}

impl ConditionallySelectable for PercentageMaxProof {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        Self {
            Y_max_proof: conditional_select_ristretto(&a.Y_max_proof, &b.Y_max_proof, choice),
            z_max_proof: Scalar::conditional_select(&a.z_max_proof, &b.z_max_proof, choice),
            c_max_proof: Scalar::conditional_select(&a.c_max_proof, &b.c_max_proof, choice),
        }
    }
}

/// The percentage equality proof.
///
/// The proof certifies that the "real" delta value commitment and the "claimed" delta value
/// commitment encode the same message.
#[allow(non_snake_case)]
#[derive(Clone, Copy)]
pub struct PercentageEqualityProof {
    Y_delta: CompressedRistretto,
    Y_claimed: CompressedRistretto,
    z_x: Scalar,
    z_delta: Scalar,
    z_claimed: Scalar,
}

impl ConditionallySelectable for PercentageEqualityProof {
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

/// Selects one of two Ristretto points in constant time.
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
    fn test_proof_above_max_proof() {
        let transfer_amount: u64 = 55;
        let max_value: u64 = 3;

        let percentage_rate: u16 = 555; // 5.55%
        let percentage_amount: u64 = 4;
        let delta: u64 = 9475; // 4*10000 - 55*555

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (percentage_commitment, percentage_opening) = Pedersen::new(max_value);

        let scalar_rate = Scalar::from(percentage_rate);
        let delta_commitment =
            &percentage_commitment * &Scalar::from(10000_u64) - &transfer_commitment * &scalar_rate;
        let delta_opening =
            &percentage_opening * &Scalar::from(10000_u64) - &transfer_opening * &scalar_rate;

        let (claimed_commitment, claimed_opening) = Pedersen::new(0_u64);

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = PercentageWithCapProof::new(
            &percentage_commitment,
            &percentage_opening,
            percentage_amount,
            &delta_commitment,
            &delta_opening,
            delta,
            &claimed_commitment,
            &claimed_opening,
            max_value,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                &percentage_commitment,
                &delta_commitment,
                &claimed_commitment,
                max_value,
                &mut verifier_transcript,
            )
            .is_ok());
    }

    #[test]
    fn test_proof_below_max_proof() {
        let transfer_amount: u64 = 1;
        let max_value: u64 = 3;

        let percentage_rate: u16 = 400; // 5.55%
        let percentage_amount: u64 = 1;
        let delta: u64 = 9600; // 4*10000 - 55*555

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (percentage_commitment, percentage_opening) = Pedersen::new(percentage_amount);

        let scalar_rate = Scalar::from(percentage_rate);
        let delta_commitment =
            &percentage_commitment * &Scalar::from(10000_u64) - &transfer_commitment * &scalar_rate;
        let delta_opening =
            &percentage_opening * &Scalar::from(10000_u64) - &transfer_opening * &scalar_rate;

        let (claimed_commitment, claimed_opening) = Pedersen::new(delta);

        assert_eq!(
            delta_commitment.get_point() - delta_opening.get_scalar() * &(*H),
            claimed_commitment.get_point() - claimed_opening.get_scalar() * &(*H)
        );

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = PercentageWithCapProof::new(
            &percentage_commitment,
            &percentage_opening,
            percentage_amount,
            &delta_commitment,
            &delta_opening,
            delta,
            &claimed_commitment,
            &claimed_opening,
            max_value,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                &percentage_commitment,
                &delta_commitment,
                &claimed_commitment,
                max_value,
                &mut verifier_transcript,
            )
            .is_ok());
    }

    #[test]
    fn test_proof_delta_is_zero() {
        let transfer_amount: u64 = 100;
        let max_value: u64 = 3;

        let percentage_rate: u16 = 100; // 1.00%
        let percentage_amount: u64 = 1;
        let delta: u64 = 0; // 1*10000 - 100*100

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (percentage_commitment, percentage_opening) = Pedersen::new(percentage_amount);

        let scalar_rate = Scalar::from(percentage_rate);
        let delta_commitment = &(&percentage_commitment * &Scalar::from(10000_u64))
            - &(&transfer_commitment * &scalar_rate);
        let delta_opening =
            &(&percentage_opening * &Scalar::from(10000_u64)) - &(&transfer_opening * &scalar_rate);

        let (claimed_commitment, claimed_opening) = Pedersen::new(delta);

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = PercentageWithCapProof::new(
            &percentage_commitment,
            &percentage_opening,
            percentage_amount,
            &delta_commitment,
            &delta_opening,
            delta,
            &claimed_commitment,
            &claimed_opening,
            max_value,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                &percentage_commitment,
                &delta_commitment,
                &claimed_commitment,
                max_value,
                &mut verifier_transcript,
            )
            .is_ok());
    }
}
