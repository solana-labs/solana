#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::pedersen::{Pedersen, PedersenOpen},
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
    subtle::{Choice, ConditionallySelectable},
};
use {
    crate::{
        encryption::pedersen::PedersenBase, errors::ProofError,
        range_proof::generators::BulletproofGens, range_proof::inner_product::InnerProductProof,
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
    #[allow(clippy::many_single_char_names)]
    #[cfg(not(target_arch = "bpf"))]
    pub fn create(
        amounts: Vec<u64>,
        bit_lengths: Vec<usize>,
        opens: Vec<&PedersenOpen>,
        transcript: &mut Transcript,
    ) -> Self {
        let t_1_blinding = PedersenOpen::random(&mut OsRng);
        let t_2_blinding = PedersenOpen::random(&mut OsRng);

        let (range_proof, _, _) = Self::create_with(
            amounts,
            bit_lengths,
            opens,
            &t_1_blinding,
            &t_2_blinding,
            transcript,
        );

        range_proof
    }

    #[allow(clippy::many_single_char_names)]
    #[cfg(not(target_arch = "bpf"))]
    pub fn create_with(
        amounts: Vec<u64>,
        bit_lengths: Vec<usize>,
        opens: Vec<&PedersenOpen>,
        t_1_blinding: &PedersenOpen,
        t_2_blinding: &PedersenOpen,
        transcript: &mut Transcript,
    ) -> (Self, Scalar, Scalar) {
        let nm = bit_lengths.iter().sum();

        // Computing the generators online for now. It should ultimately be precomputed.
        let bp_gens = BulletproofGens::new(nm);
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;

        // bit-decompose values and commit to the bits
        let a_blinding = Scalar::random(&mut OsRng);
        let mut A = a_blinding * H;

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

        // generate blinding factors and commit as vectors
        let s_blinding = Scalar::random(&mut OsRng);

        let s_L: Vec<Scalar> = (0..nm).map(|_| Scalar::random(&mut OsRng)).collect();
        let s_R: Vec<Scalar> = (0..nm).map(|_| Scalar::random(&mut OsRng)).collect();

        let S = RistrettoPoint::multiscalar_mul(
            iter::once(&s_blinding).chain(s_L.iter()).chain(s_R.iter()),
            iter::once(&H).chain(bp_gens.G(nm)).chain(bp_gens.H(nm)),
        );

        transcript.append_point(b"A", &A.compress());
        transcript.append_point(b"S", &S.compress());

        // commit to T1 and T2
        let y = transcript.challenge_scalar(b"y");
        let z = transcript.challenge_scalar(b"z");

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

        let t_poly = l_poly.inner_product(&r_poly);

        let T_1 = Pedersen::commit_with(t_poly.1, t_1_blinding)
            .get_point()
            .compress();
        let T_2 = Pedersen::commit_with(t_poly.2, t_2_blinding)
            .get_point()
            .compress();

        transcript.append_point(b"T_1", &T_1);
        transcript.append_point(b"T_2", &T_2);

        let x = transcript.challenge_scalar(b"x");

        let mut agg_open = Scalar::zero();
        let mut exp_z = z * z;
        for open in opens {
            agg_open += exp_z * open.get_scalar();
            exp_z *= z;
        }

        let t_blinding_poly = util::Poly2(
            agg_open,
            t_1_blinding.get_scalar(),
            t_2_blinding.get_scalar(),
        );

        // compute t_x
        let t_x = t_poly.eval(x);
        let t_x_blinding = t_blinding_poly.eval(x);

        let e_blinding = a_blinding + s_blinding * x;
        let l_vec = l_poly.eval(x);
        let r_vec = r_poly.eval(x);

        transcript.append_scalar(b"t_x", &t_x);
        transcript.append_scalar(b"t_x_blinding", &t_x_blinding);
        transcript.append_scalar(b"e_blinding", &e_blinding);

        let w = transcript.challenge_scalar(b"w");
        let Q = w * G;

        transcript.challenge_scalar(b"c");

        let G_factors: Vec<Scalar> = iter::repeat(Scalar::one()).take(nm).collect();
        let H_factors: Vec<Scalar> = util::exp_iter(y.invert()).take(nm).collect();

        let ipp_proof = InnerProductProof::create(
            &Q,
            &G_factors,
            &H_factors,
            bp_gens.G(nm).cloned().collect(),
            bp_gens.H(nm).cloned().collect(),
            l_vec,
            r_vec,
            transcript,
        );

        let range_proof = RangeProof {
            A: A.compress(),
            S: S.compress(),
            T_1,
            T_2,
            t_x,
            t_x_blinding,
            e_blinding,
            ipp_proof,
        };

        (range_proof, x, z)
    }

    #[allow(clippy::many_single_char_names)]
    pub fn verify(
        &self,
        comms: Vec<&CompressedRistretto>,
        bit_lengths: Vec<usize>,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        self.verify_with(comms, bit_lengths, None, None, transcript)
    }

    #[allow(clippy::many_single_char_names)]
    pub fn verify_with(
        &self,
        comms: Vec<&CompressedRistretto>,
        bit_lengths: Vec<usize>,
        x_ver: Option<Scalar>,
        z_ver: Option<Scalar>,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;

        let m = bit_lengths.len();
        let nm: usize = bit_lengths.iter().sum();
        let bp_gens = BulletproofGens::new(nm);

        if !(nm == 8 || nm == 16 || nm == 32 || nm == 64 || nm == 128) {
            return Err(ProofError::InvalidBitsize);
        }

        transcript.validate_and_append_point(b"A", &self.A)?;
        transcript.validate_and_append_point(b"S", &self.S)?;

        let y = transcript.challenge_scalar(b"y");
        let z = transcript.challenge_scalar(b"z");

        if z_ver.is_some() && z_ver.unwrap() != z {
            return Err(ProofError::VerificationError);
        }

        let zz = z * z;
        let minus_z = -z;

        transcript.validate_and_append_point(b"T_1", &self.T_1)?;
        transcript.validate_and_append_point(b"T_2", &self.T_2)?;

        let x = transcript.challenge_scalar(b"x");

        if x_ver.is_some() && x_ver.unwrap() != x {
            return Err(ProofError::VerificationError);
        }

        transcript.append_scalar(b"t_x", &self.t_x);
        transcript.append_scalar(b"t_x_blinding", &self.t_x_blinding);
        transcript.append_scalar(b"e_blinding", &self.e_blinding);

        let w = transcript.challenge_scalar(b"w");

        // Challenge value for batching statements to be verified
        let c = transcript.challenge_scalar(b"c");

        let (x_sq, x_inv_sq, s) = self.ipp_proof.verification_scalars(nm, transcript)?;
        let s_inv = s.iter().rev();

        let a = self.ipp_proof.a;
        let b = self.ipp_proof.b;

        // Construct concat_z_and_2, an iterator of the values of
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
            .clone()
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
                .chain(iter::once(Some(H)))
                .chain(iter::once(Some(G)))
                .chain(self.ipp_proof.L_vec.iter().map(|L| L.decompress()))
                .chain(self.ipp_proof.R_vec.iter().map(|R| R.decompress()))
                .chain(bp_gens.G(nm).map(|&x| Some(x)))
                .chain(bp_gens.H(nm).map(|&x| Some(x)))
                .chain(comms.iter().map(|V| V.decompress())),
        )
        .ok_or(ProofError::VerificationError)?;

        if mega_check.is_identity() {
            Ok(())
        } else {
            Err(ProofError::VerificationError)
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
    pub fn from_bytes(slice: &[u8]) -> Result<RangeProof, ProofError> {
        if slice.len() % 32 != 0 {
            return Err(ProofError::FormatError);
        }
        if slice.len() < 7 * 32 {
            return Err(ProofError::FormatError);
        }

        let A = CompressedRistretto(util::read32(&slice[0..]));
        let S = CompressedRistretto(util::read32(&slice[32..]));
        let T_1 = CompressedRistretto(util::read32(&slice[2 * 32..]));
        let T_2 = CompressedRistretto(util::read32(&slice[3 * 32..]));

        let t_x = Scalar::from_canonical_bytes(util::read32(&slice[4 * 32..]))
            .ok_or(ProofError::FormatError)?;
        let t_x_blinding = Scalar::from_canonical_bytes(util::read32(&slice[5 * 32..]))
            .ok_or(ProofError::FormatError)?;
        let e_blinding = Scalar::from_canonical_bytes(util::read32(&slice[6 * 32..]))
            .ok_or(ProofError::FormatError)?;

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
        let (comm, open) = Pedersen::commit(55_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::create(vec![55], vec![32], vec![&open], &mut transcript_create);

        assert!(proof
            .verify(
                vec![&comm.get_point().compress()],
                vec![32],
                &mut transcript_verify
            )
            .is_ok());
    }

    #[test]
    fn test_aggregated_rangeproof() {
        let (comm_1, open_1) = Pedersen::commit(55_u64);
        let (comm_2, open_2) = Pedersen::commit(77_u64);
        let (comm_3, open_3) = Pedersen::commit(99_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::create(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        );

        let comm_1_point = comm_1.get_point().compress();
        let comm_2_point = comm_2.get_point().compress();
        let comm_3_point = comm_3.get_point().compress();

        assert!(proof
            .verify(
                vec![&comm_1_point, &comm_2_point, &comm_3_point],
                vec![64, 32, 32],
                &mut transcript_verify,
            )
            .is_ok());
    }

    // TODO: write test for serialization/deserialization
}
