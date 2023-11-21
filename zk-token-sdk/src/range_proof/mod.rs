#[cfg(not(target_os = "solana"))]
use {
    crate::encryption::pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
    subtle::{Choice, ConditionallySelectable},
};
use {
    crate::{
        encryption::pedersen::{G, H},
        errors::ProofVerificationError,
        range_proof::{
            errors::{RangeProofError, RangeProofGenerationError},
            generators::BulletproofGens,
            inner_product::InnerProductProof,
        },
        transcript::TranscriptProtocol,
    },
    core::iter,
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

pub mod errors;
pub mod generators;
pub mod inner_product;
pub mod util;

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct RangeProof {
    pub A: CompressedRistretto,       // 32 bytes
    pub S: CompressedRistretto,       // 32 bytes
    pub T_1: CompressedRistretto,     // 32 bytes
    pub T_2: CompressedRistretto,     // 32 bytes
    pub t_x: Scalar,                  // 32 bytes
    pub t_x_blinding: Scalar,         // 32 bytes
    pub e_blinding: Scalar,           // 32 bytes
    pub ipp_proof: InnerProductProof, // 448 bytes for withdraw; 512 for transfer
}

#[allow(non_snake_case)]
impl RangeProof {
    /// Create an aggregated range proof.
    ///
    /// The proof is created with respect to a vector of Pedersen commitments C_1, ..., C_m. The
    /// method itself does not take in these commitments, but the values associated with the commitments:
    /// - vector of committed amounts v_1, ..., v_m represented as u64
    /// - bit-lengths of the committed amounts
    /// - Pedersen openings for each commitments
    ///
    /// The sum of the bit-lengths of the commitments amounts must be a power-of-two
    #[allow(clippy::many_single_char_names)]
    #[cfg(not(target_os = "solana"))]
    pub fn new(
        amounts: Vec<u64>,
        bit_lengths: Vec<usize>,
        openings: Vec<&PedersenOpening>,
        transcript: &mut Transcript,
    ) -> Result<Self, RangeProofGenerationError> {
        // amounts, bit-lengths, openings must be same length vectors
        let m = amounts.len();
        assert_eq!(bit_lengths.len(), m);
        assert_eq!(openings.len(), m);

        // total vector dimension to compute the ultimate inner product proof for
        let nm: usize = bit_lengths.iter().sum();
        assert!(nm.is_power_of_two());

        let bp_gens = BulletproofGens::new(nm)
            .map_err(|_| RangeProofGenerationError::MaximumGeneratorLengthExceeded)?;

        // bit-decompose values and generate their Pedersen vector commitment
        let a_blinding = Scalar::random(&mut OsRng);
        let mut A = a_blinding * &(*H);

        let mut gens_iter = bp_gens.G(nm).zip(bp_gens.H(nm));
        for (amount_i, n_i) in amounts.iter().zip(bit_lengths.iter()) {
            for j in 0..(*n_i) {
                let (G_ij, H_ij) = gens_iter.next().unwrap();
                let v_ij = Choice::from(((amount_i >> j) & 1) as u8);
                let mut point = -H_ij;
                point.conditional_assign(G_ij, v_ij);
                A += point;
            }
        }
        let A = A.compress();

        // generate blinding factors and generate their Pedersen vector commitment
        let s_L: Vec<Scalar> = (0..nm).map(|_| Scalar::random(&mut OsRng)).collect();
        let s_R: Vec<Scalar> = (0..nm).map(|_| Scalar::random(&mut OsRng)).collect();

        // generate blinding factor for Pedersen commitment; `s_blinding` should not to be confused
        // with blinding factors for the actual inner product vector
        let s_blinding = Scalar::random(&mut OsRng);

        let S = RistrettoPoint::multiscalar_mul(
            iter::once(&s_blinding).chain(s_L.iter()).chain(s_R.iter()),
            iter::once(&(*H)).chain(bp_gens.G(nm)).chain(bp_gens.H(nm)),
        )
        .compress();

        // add the Pedersen vector commitments to the transcript (send the commitments to the verifier)
        transcript.append_point(b"A", &A);
        transcript.append_point(b"S", &S);

        // derive challenge scalars from the transcript (receive challenge from the verifier): `y`
        // and `z` used for merge multiple inner product relations into one single inner product
        let y = transcript.challenge_scalar(b"y");
        let z = transcript.challenge_scalar(b"z");

        // define blinded vectors:
        // - l(x) = (a_L - z*1) + s_L*x
        // - r(x) = (y^n * (a_R + z*1) + [z^2*2^n | z^3*2^n | ... | z^m*2^n]) + y^n * s_R*x
        let mut l_poly = util::VecPoly1::zero(nm);
        let mut r_poly = util::VecPoly1::zero(nm);

        let mut i = 0;
        let mut exp_z = z * z;
        let mut exp_y = Scalar::one();

        for (amount_i, n_i) in amounts.iter().zip(bit_lengths.iter()) {
            let mut exp_2 = Scalar::one();

            for j in 0..(*n_i) {
                let a_L_j = Scalar::from((amount_i >> j) & 1);
                let a_R_j = a_L_j - Scalar::one();

                l_poly.0[i] = a_L_j - z;
                l_poly.1[i] = s_L[i];
                r_poly.0[i] = exp_y * (a_R_j + z) + exp_z * exp_2;
                r_poly.1[i] = exp_y * s_R[i];

                exp_y *= y;
                exp_2 = exp_2 + exp_2;
                i += 1;
            }
            exp_z *= z;
        }

        // define t(x) = <l(x), r(x)> = t_0 + t_1*x + t_2*x
        let t_poly = l_poly.inner_product(&r_poly);

        // generate Pedersen commitment for the coefficients t_1 and t_2
        let (T_1, t_1_blinding) = Pedersen::new(t_poly.1);
        let (T_2, t_2_blinding) = Pedersen::new(t_poly.2);

        let T_1 = T_1.get_point().compress();
        let T_2 = T_2.get_point().compress();

        transcript.append_point(b"T_1", &T_1);
        transcript.append_point(b"T_2", &T_2);

        // evaluate t(x) on challenge x and homomorphically compute the openings for
        // z^2 * V_1 + z^3 * V_2 + ... + z^{m+1} * V_m + delta(y, z)*G + x*T_1 + x^2*T_2
        let x = transcript.challenge_scalar(b"x");

        let mut agg_opening = Scalar::zero();
        let mut exp_z = z;
        for opening in openings {
            exp_z *= z;
            agg_opening += exp_z * opening.get_scalar();
        }

        let t_blinding_poly = util::Poly2(
            agg_opening,
            *t_1_blinding.get_scalar(),
            *t_2_blinding.get_scalar(),
        );

        let t_x = t_poly.eval(x);
        let t_x_blinding = t_blinding_poly.eval(x);

        transcript.append_scalar(b"t_x", &t_x);
        transcript.append_scalar(b"t_x_blinding", &t_x_blinding);

        // homomorphically compuate the openings for A + x*S
        let e_blinding = a_blinding + s_blinding * x;
        let l_vec = l_poly.eval(x);
        let r_vec = r_poly.eval(x);

        transcript.append_scalar(b"e_blinding", &e_blinding);

        // compute the inner product argument on the commitment:
        // P = <l(x), G> + <r(x), H'> + <l(x), r(x)>*Q
        let w = transcript.challenge_scalar(b"w");
        let Q = w * &(*G);

        let G_factors: Vec<Scalar> = iter::repeat(Scalar::one()).take(nm).collect();
        let H_factors: Vec<Scalar> = util::exp_iter(y.invert()).take(nm).collect();

        // generate challenge `c` for consistency with the verifier's transcript
        transcript.challenge_scalar(b"c");

        let ipp_proof = InnerProductProof::new(
            &Q,
            &G_factors,
            &H_factors,
            bp_gens.G(nm).cloned().collect(),
            bp_gens.H(nm).cloned().collect(),
            l_vec,
            r_vec,
            transcript,
        );

        Ok(RangeProof {
            A,
            S,
            T_1,
            T_2,
            t_x,
            t_x_blinding,
            e_blinding,
            ipp_proof,
        })
    }

    #[allow(clippy::many_single_char_names)]
    pub fn verify(
        &self,
        comms: Vec<&PedersenCommitment>,
        bit_lengths: Vec<usize>,
        transcript: &mut Transcript,
    ) -> Result<(), RangeProofError> {
        // commitments and bit-lengths must be same length vectors
        assert_eq!(comms.len(), bit_lengths.len());

        let m = bit_lengths.len();
        let nm: usize = bit_lengths.iter().sum();
        let bp_gens = BulletproofGens::new(nm)
            .map_err(|_| ProofVerificationError::InvalidGeneratorsLength)?;

        if !nm.is_power_of_two() {
            return Err(ProofVerificationError::InvalidBitSize.into());
        }

        // append proof data to transcript and derive appropriate challenge scalars
        transcript.validate_and_append_point(b"A", &self.A)?;
        transcript.validate_and_append_point(b"S", &self.S)?;

        let y = transcript.challenge_scalar(b"y");
        let z = transcript.challenge_scalar(b"z");

        let zz = z * z;
        let minus_z = -z;

        transcript.validate_and_append_point(b"T_1", &self.T_1)?;
        transcript.validate_and_append_point(b"T_2", &self.T_2)?;

        let x = transcript.challenge_scalar(b"x");

        transcript.append_scalar(b"t_x", &self.t_x);
        transcript.append_scalar(b"t_x_blinding", &self.t_x_blinding);
        transcript.append_scalar(b"e_blinding", &self.e_blinding);

        let w = transcript.challenge_scalar(b"w");
        let c = transcript.challenge_scalar(b"c"); // challenge value for batching multiscalar mul

        // verify inner product proof
        let (x_sq, x_inv_sq, s) = self.ipp_proof.verification_scalars(nm, transcript)?;
        let s_inv = s.iter().rev();

        let a = self.ipp_proof.a;
        let b = self.ipp_proof.b;

        // construct concat_z_and_2, an iterator of the values of
        // z^0 * \vec(2)^n || z^1 * \vec(2)^n || ... || z^(m-1) * \vec(2)^n
        let concat_z_and_2: Vec<Scalar> = util::exp_iter(z)
            .zip(bit_lengths.iter())
            .flat_map(|(exp_z, n_i)| {
                util::exp_iter(Scalar::from(2u64))
                    .take(*n_i)
                    .map(move |exp_2| exp_2 * exp_z)
            })
            .collect();

        let gs = s.iter().map(|s_i| minus_z - a * s_i);
        let hs = s_inv
            .zip(util::exp_iter(y.invert()))
            .zip(concat_z_and_2.iter())
            .map(|((s_i_inv, exp_y_inv), z_and_2)| z + exp_y_inv * (zz * z_and_2 - b * s_i_inv));

        let basepoint_scalar =
            w * (self.t_x - a * b) + c * (delta(&bit_lengths, &y, &z) - self.t_x);
        let value_commitment_scalars = util::exp_iter(z).take(m).map(|z_exp| c * zz * z_exp);

        let mega_check = RistrettoPoint::optional_multiscalar_mul(
            iter::once(Scalar::one())
                .chain(iter::once(x))
                .chain(iter::once(c * x))
                .chain(iter::once(c * x * x))
                .chain(iter::once(-self.e_blinding - c * self.t_x_blinding))
                .chain(iter::once(basepoint_scalar))
                .chain(x_sq.iter().cloned())
                .chain(x_inv_sq.iter().cloned())
                .chain(gs)
                .chain(hs)
                .chain(value_commitment_scalars),
            iter::once(self.A.decompress())
                .chain(iter::once(self.S.decompress()))
                .chain(iter::once(self.T_1.decompress()))
                .chain(iter::once(self.T_2.decompress()))
                .chain(iter::once(Some(*H)))
                .chain(iter::once(Some(*G)))
                .chain(self.ipp_proof.L_vec.iter().map(|L| L.decompress()))
                .chain(self.ipp_proof.R_vec.iter().map(|R| R.decompress()))
                .chain(bp_gens.G(nm).map(|&x| Some(x)))
                .chain(bp_gens.H(nm).map(|&x| Some(x)))
                .chain(comms.iter().map(|V| Some(*V.get_point()))),
        )
        .ok_or(ProofVerificationError::MultiscalarMul)?;

        if mega_check.is_identity() {
            Ok(())
        } else {
            Err(ProofVerificationError::AlgebraicRelation.into())
        }
    }

    // Following the dalek rangeproof library signature for now. The exact method signature can be
    // changed.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(7 * 32 + self.ipp_proof.serialized_size());
        buf.extend_from_slice(self.A.as_bytes());
        buf.extend_from_slice(self.S.as_bytes());
        buf.extend_from_slice(self.T_1.as_bytes());
        buf.extend_from_slice(self.T_2.as_bytes());
        buf.extend_from_slice(self.t_x.as_bytes());
        buf.extend_from_slice(self.t_x_blinding.as_bytes());
        buf.extend_from_slice(self.e_blinding.as_bytes());
        buf.extend_from_slice(&self.ipp_proof.to_bytes());
        buf
    }

    // Following the dalek rangeproof library signature for now. The exact method signature can be
    // changed.
    pub fn from_bytes(slice: &[u8]) -> Result<RangeProof, RangeProofError> {
        if slice.len() % 32 != 0 {
            return Err(ProofVerificationError::Deserialization.into());
        }
        if slice.len() < 7 * 32 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let A = CompressedRistretto(util::read32(&slice[0..]));
        let S = CompressedRistretto(util::read32(&slice[32..]));
        let T_1 = CompressedRistretto(util::read32(&slice[2 * 32..]));
        let T_2 = CompressedRistretto(util::read32(&slice[3 * 32..]));

        let t_x = Scalar::from_canonical_bytes(util::read32(&slice[4 * 32..]))
            .ok_or(ProofVerificationError::Deserialization)?;
        let t_x_blinding = Scalar::from_canonical_bytes(util::read32(&slice[5 * 32..]))
            .ok_or(ProofVerificationError::Deserialization)?;
        let e_blinding = Scalar::from_canonical_bytes(util::read32(&slice[6 * 32..]))
            .ok_or(ProofVerificationError::Deserialization)?;

        let ipp_proof = InnerProductProof::from_bytes(&slice[7 * 32..])?;

        Ok(RangeProof {
            A,
            S,
            T_1,
            T_2,
            t_x,
            t_x_blinding,
            e_blinding,
            ipp_proof,
        })
    }
}

/// Compute
/// \\[
/// \delta(y,z) = (z - z^{2}) \langle \mathbf{1}, {\mathbf{y}}^{n \cdot m} \rangle - \sum_{j=0}^{m-1} z^{j+3} \cdot \langle \mathbf{1}, {\mathbf{2}}^{n \cdot m} \rangle
/// \\]
fn delta(bit_lengths: &[usize], y: &Scalar, z: &Scalar) -> Scalar {
    let nm: usize = bit_lengths.iter().sum();
    let sum_y = util::sum_of_powers(y, nm);

    let mut agg_delta = (z - z * z) * sum_y;
    let mut exp_z = z * z * z;
    for n_i in bit_lengths.iter() {
        let sum_2 = util::sum_of_powers(&Scalar::from(2u64), *n_i);
        agg_delta -= exp_z * sum_2;
        exp_z *= z;
    }
    agg_delta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_rangeproof() {
        let (comm, open) = Pedersen::new(55_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof =
            RangeProof::new(vec![55], vec![32], vec![&open], &mut transcript_create).unwrap();

        assert!(proof
            .verify(vec![&comm], vec![32], &mut transcript_verify)
            .is_ok());
    }

    #[test]
    fn test_aggregated_rangeproof() {
        let (comm_1, open_1) = Pedersen::new(55_u64);
        let (comm_2, open_2) = Pedersen::new(77_u64);
        let (comm_3, open_3) = Pedersen::new(99_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::new(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        )
        .unwrap();

        assert!(proof
            .verify(
                vec![&comm_1, &comm_2, &comm_3],
                vec![64, 32, 32],
                &mut transcript_verify,
            )
            .is_ok());
    }

    // TODO: write test for serialization/deserialization
}
