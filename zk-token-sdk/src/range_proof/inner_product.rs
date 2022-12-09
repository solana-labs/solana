use {
    crate::{
        errors::ProofVerificationError,
        range_proof::{errors::RangeProofError, util},
        transcript::TranscriptProtocol,
    },
    core::iter,
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{MultiscalarMul, VartimeMultiscalarMul},
    },
    merlin::Transcript,
    std::borrow::Borrow,
};

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct InnerProductProof {
    pub L_vec: Vec<CompressedRistretto>, // 32 * log(bit_length)
    pub R_vec: Vec<CompressedRistretto>, // 32 * log(bit_length)
    pub a: Scalar,                       // 32 bytes
    pub b: Scalar,                       // 32 bytes
}

#[allow(non_snake_case)]
impl InnerProductProof {
    /// Create an inner-product proof.
    ///
    /// The proof is created with respect to the bases \\(G\\), \\(H'\\),
    /// where \\(H'\_i = H\_i \cdot \texttt{Hprime\\_factors}\_i\\).
    ///
    /// The `verifier` is passed in as a parameter so that the
    /// challenges depend on the *entire* transcript (including parent
    /// protocols).
    ///
    /// The lengths of the vectors must all be the same, and must all be
    /// a power of 2.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        Q: &RistrettoPoint,
        G_factors: &[Scalar],
        H_factors: &[Scalar],
        mut G_vec: Vec<RistrettoPoint>,
        mut H_vec: Vec<RistrettoPoint>,
        mut a_vec: Vec<Scalar>,
        mut b_vec: Vec<Scalar>,
        transcript: &mut Transcript,
    ) -> Self {
        // Create slices G, H, a, b backed by their respective
        // vectors.  This lets us reslice as we compress the lengths
        // of the vectors in the main loop below.
        let mut G = &mut G_vec[..];
        let mut H = &mut H_vec[..];
        let mut a = &mut a_vec[..];
        let mut b = &mut b_vec[..];

        let mut n = G.len();

        // All of the input vectors must have the same length.
        assert_eq!(G.len(), n);
        assert_eq!(H.len(), n);
        assert_eq!(a.len(), n);
        assert_eq!(b.len(), n);
        assert_eq!(G_factors.len(), n);
        assert_eq!(H_factors.len(), n);

        // All of the input vectors must have a length that is a power of two.
        assert!(n.is_power_of_two());

        transcript.innerproduct_domain_sep(n as u64);

        let lg_n = n.next_power_of_two().trailing_zeros() as usize;
        let mut L_vec = Vec::with_capacity(lg_n);
        let mut R_vec = Vec::with_capacity(lg_n);

        // If it's the first iteration, unroll the Hprime = H*y_inv scalar mults
        // into multiscalar muls, for performance.
        if n != 1 {
            n /= 2;
            let (a_L, a_R) = a.split_at_mut(n);
            let (b_L, b_R) = b.split_at_mut(n);
            let (G_L, G_R) = G.split_at_mut(n);
            let (H_L, H_R) = H.split_at_mut(n);

            let c_L = util::inner_product(a_L, b_R);
            let c_R = util::inner_product(a_R, b_L);

            let L = RistrettoPoint::multiscalar_mul(
                a_L.iter()
                    .zip(G_factors[n..2 * n].iter())
                    .map(|(a_L_i, g)| a_L_i * g)
                    .chain(
                        b_R.iter()
                            .zip(H_factors[0..n].iter())
                            .map(|(b_R_i, h)| b_R_i * h),
                    )
                    .chain(iter::once(c_L)),
                G_R.iter().chain(H_L.iter()).chain(iter::once(Q)),
            )
            .compress();

            let R = RistrettoPoint::multiscalar_mul(
                a_R.iter()
                    .zip(G_factors[0..n].iter())
                    .map(|(a_R_i, g)| a_R_i * g)
                    .chain(
                        b_L.iter()
                            .zip(H_factors[n..2 * n].iter())
                            .map(|(b_L_i, h)| b_L_i * h),
                    )
                    .chain(iter::once(c_R)),
                G_L.iter().chain(H_R.iter()).chain(iter::once(Q)),
            )
            .compress();

            L_vec.push(L);
            R_vec.push(R);

            transcript.append_point(b"L", &L);
            transcript.append_point(b"R", &R);

            let u = transcript.challenge_scalar(b"u");
            let u_inv = u.invert();

            for i in 0..n {
                a_L[i] = a_L[i] * u + u_inv * a_R[i];
                b_L[i] = b_L[i] * u_inv + u * b_R[i];
                G_L[i] = RistrettoPoint::multiscalar_mul(
                    &[u_inv * G_factors[i], u * G_factors[n + i]],
                    &[G_L[i], G_R[i]],
                );
                H_L[i] = RistrettoPoint::multiscalar_mul(
                    &[u * H_factors[i], u_inv * H_factors[n + i]],
                    &[H_L[i], H_R[i]],
                )
            }

            a = a_L;
            b = b_L;
            G = G_L;
            H = H_L;
        }

        while n != 1 {
            n /= 2;
            let (a_L, a_R) = a.split_at_mut(n);
            let (b_L, b_R) = b.split_at_mut(n);
            let (G_L, G_R) = G.split_at_mut(n);
            let (H_L, H_R) = H.split_at_mut(n);

            let c_L = util::inner_product(a_L, b_R);
            let c_R = util::inner_product(a_R, b_L);

            let L = RistrettoPoint::multiscalar_mul(
                a_L.iter().chain(b_R.iter()).chain(iter::once(&c_L)),
                G_R.iter().chain(H_L.iter()).chain(iter::once(Q)),
            )
            .compress();

            let R = RistrettoPoint::multiscalar_mul(
                a_R.iter().chain(b_L.iter()).chain(iter::once(&c_R)),
                G_L.iter().chain(H_R.iter()).chain(iter::once(Q)),
            )
            .compress();

            L_vec.push(L);
            R_vec.push(R);

            transcript.append_point(b"L", &L);
            transcript.append_point(b"R", &R);

            let u = transcript.challenge_scalar(b"u");
            let u_inv = u.invert();

            for i in 0..n {
                a_L[i] = a_L[i] * u + u_inv * a_R[i];
                b_L[i] = b_L[i] * u_inv + u * b_R[i];
                G_L[i] = RistrettoPoint::multiscalar_mul(&[u_inv, u], &[G_L[i], G_R[i]]);
                H_L[i] = RistrettoPoint::multiscalar_mul(&[u, u_inv], &[H_L[i], H_R[i]]);
            }

            a = a_L;
            b = b_L;
            G = G_L;
            H = H_L;
        }

        InnerProductProof {
            L_vec,
            R_vec,
            a: a[0],
            b: b[0],
        }
    }

    /// Computes three vectors of verification scalars \\([u\_{i}^{2}]\\), \\([u\_{i}^{-2}]\\) and
    /// \\([s\_{i}]\\) for combined multiscalar multiplication in a parent protocol. See [inner
    /// product protocol notes](index.html#verification-equation) for details. The verifier must
    /// provide the input length \\(n\\) explicitly to avoid unbounded allocation within the inner
    /// product proof.
    #[allow(clippy::type_complexity)]
    pub(crate) fn verification_scalars(
        &self,
        n: usize,
        transcript: &mut Transcript,
    ) -> Result<(Vec<Scalar>, Vec<Scalar>, Vec<Scalar>), RangeProofError> {
        let lg_n = self.L_vec.len();
        if lg_n >= 32 {
            // 4 billion multiplications should be enough for anyone
            // and this check prevents overflow in 1<<lg_n below.
            return Err(ProofVerificationError::InvalidBitSize.into());
        }
        if n != (1 << lg_n) {
            return Err(ProofVerificationError::InvalidBitSize.into());
        }

        transcript.innerproduct_domain_sep(n as u64);

        // 1. Recompute x_k,...,x_1 based on the proof transcript

        let mut challenges = Vec::with_capacity(lg_n);
        for (L, R) in self.L_vec.iter().zip(self.R_vec.iter()) {
            transcript.validate_and_append_point(b"L", L)?;
            transcript.validate_and_append_point(b"R", R)?;
            challenges.push(transcript.challenge_scalar(b"u"));
        }

        // 2. Compute 1/(u_k...u_1) and 1/u_k, ..., 1/u_1

        let mut challenges_inv = challenges.clone();
        let allinv = Scalar::batch_invert(&mut challenges_inv);

        // 3. Compute u_i^2 and (1/u_i)^2

        for i in 0..lg_n {
            challenges[i] = challenges[i] * challenges[i];
            challenges_inv[i] = challenges_inv[i] * challenges_inv[i];
        }
        let challenges_sq = challenges;
        let challenges_inv_sq = challenges_inv;

        // 4. Compute s values inductively.

        let mut s = Vec::with_capacity(n);
        s.push(allinv);
        for i in 1..n {
            let lg_i = (32 - 1 - (i as u32).leading_zeros()) as usize;
            let k = 1 << lg_i;
            // The challenges are stored in "creation order" as [u_k,...,u_1],
            // so u_{lg(i)+1} = is indexed by (lg_n-1) - lg_i
            let u_lg_i_sq = challenges_sq[(lg_n - 1) - lg_i];
            s.push(s[i - k] * u_lg_i_sq);
        }

        Ok((challenges_sq, challenges_inv_sq, s))
    }

    /// This method is for testing that proof generation work, but for efficiency the actual
    /// protocols would use `verification_scalars` method to combine inner product verification
    /// with other checks in a single multiscalar multiplication.
    #[allow(clippy::too_many_arguments)]
    pub fn verify<IG, IH>(
        &self,
        n: usize,
        G_factors: IG,
        H_factors: IH,
        P: &RistrettoPoint,
        Q: &RistrettoPoint,
        G: &[RistrettoPoint],
        H: &[RistrettoPoint],
        transcript: &mut Transcript,
    ) -> Result<(), RangeProofError>
    where
        IG: IntoIterator,
        IG::Item: Borrow<Scalar>,
        IH: IntoIterator,
        IH::Item: Borrow<Scalar>,
    {
        let (u_sq, u_inv_sq, s) = self.verification_scalars(n, transcript)?;

        let g_times_a_times_s = G_factors
            .into_iter()
            .zip(s.iter())
            .map(|(g_i, s_i)| (self.a * s_i) * g_i.borrow())
            .take(G.len());

        // 1/s[i] is s[!i], and !i runs from n-1 to 0 as i runs from 0 to n-1
        let inv_s = s.iter().rev();

        let h_times_b_div_s = H_factors
            .into_iter()
            .zip(inv_s)
            .map(|(h_i, s_i_inv)| (self.b * s_i_inv) * h_i.borrow());

        let neg_u_sq = u_sq.iter().map(|ui| -ui);
        let neg_u_inv_sq = u_inv_sq.iter().map(|ui| -ui);

        let Ls = self
            .L_vec
            .iter()
            .map(|p| {
                p.decompress()
                    .ok_or(ProofVerificationError::Deserialization)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let Rs = self
            .R_vec
            .iter()
            .map(|p| {
                p.decompress()
                    .ok_or(ProofVerificationError::Deserialization)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let expect_P = RistrettoPoint::vartime_multiscalar_mul(
            iter::once(self.a * self.b)
                .chain(g_times_a_times_s)
                .chain(h_times_b_div_s)
                .chain(neg_u_sq)
                .chain(neg_u_inv_sq),
            iter::once(Q)
                .chain(G.iter())
                .chain(H.iter())
                .chain(Ls.iter())
                .chain(Rs.iter()),
        );

        if expect_P == *P {
            Ok(())
        } else {
            Err(ProofVerificationError::AlgebraicRelation.into())
        }
    }

    /// Returns the size in bytes required to serialize the inner
    /// product proof.
    ///
    /// For vectors of length `n` the proof size is
    /// \\(32 \cdot (2\lg n+2)\\) bytes.
    pub fn serialized_size(&self) -> usize {
        (self.L_vec.len() * 2 + 2) * 32
    }

    /// Serializes the proof into a byte array of \\(2n+2\\) 32-byte elements.
    /// The layout of the inner product proof is:
    /// * \\(n\\) pairs of compressed Ristretto points \\(L_0, R_0 \dots, L_{n-1}, R_{n-1}\\),
    /// * two scalars \\(a, b\\).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.serialized_size());
        for (l, r) in self.L_vec.iter().zip(self.R_vec.iter()) {
            buf.extend_from_slice(l.as_bytes());
            buf.extend_from_slice(r.as_bytes());
        }
        buf.extend_from_slice(self.a.as_bytes());
        buf.extend_from_slice(self.b.as_bytes());
        buf
    }

    /// Deserializes the proof from a byte slice.
    /// Returns an error in the following cases:
    /// * the slice does not have \\(2n+2\\) 32-byte elements,
    /// * \\(n\\) is larger or equal to 32 (proof is too big),
    /// * any of \\(2n\\) points are not valid compressed Ristretto points,
    /// * any of 2 scalars are not canonical scalars modulo Ristretto group order.
    pub fn from_bytes(slice: &[u8]) -> Result<InnerProductProof, RangeProofError> {
        let b = slice.len();
        if b % 32 != 0 {
            return Err(ProofVerificationError::Deserialization.into());
        }
        let num_elements = b / 32;
        if num_elements < 2 {
            return Err(ProofVerificationError::Deserialization.into());
        }
        if (num_elements - 2) % 2 != 0 {
            return Err(ProofVerificationError::Deserialization.into());
        }
        let lg_n = (num_elements - 2) / 2;
        if lg_n >= 32 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let mut L_vec: Vec<CompressedRistretto> = Vec::with_capacity(lg_n);
        let mut R_vec: Vec<CompressedRistretto> = Vec::with_capacity(lg_n);
        for i in 0..lg_n {
            let pos = 2 * i * 32;
            L_vec.push(CompressedRistretto(util::read32(&slice[pos..])));
            R_vec.push(CompressedRistretto(util::read32(&slice[pos + 32..])));
        }

        let pos = 2 * lg_n * 32;
        let a = Scalar::from_canonical_bytes(util::read32(&slice[pos..]))
            .ok_or(ProofVerificationError::Deserialization)?;
        let b = Scalar::from_canonical_bytes(util::read32(&slice[pos + 32..]))
            .ok_or(ProofVerificationError::Deserialization)?;

        Ok(InnerProductProof { L_vec, R_vec, a, b })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, crate::range_proof::generators::BulletproofGens, rand::rngs::OsRng,
        sha3::Sha3_512,
    };

    #[test]
    #[allow(non_snake_case)]
    fn test_basic_correctness() {
        let n = 32;

        let bp_gens = BulletproofGens::new(n);
        let G: Vec<RistrettoPoint> = bp_gens.G(n).cloned().collect();
        let H: Vec<RistrettoPoint> = bp_gens.H(n).cloned().collect();

        let Q = RistrettoPoint::hash_from_bytes::<Sha3_512>(b"test point");

        let a: Vec<_> = (0..n).map(|_| Scalar::random(&mut OsRng)).collect();
        let b: Vec<_> = (0..n).map(|_| Scalar::random(&mut OsRng)).collect();
        let c = util::inner_product(&a, &b);

        let G_factors: Vec<Scalar> = iter::repeat(Scalar::one()).take(n).collect();

        let y_inv = Scalar::random(&mut OsRng);
        let H_factors: Vec<Scalar> = util::exp_iter(y_inv).take(n).collect();

        // P would be determined upstream, but we need a correct P to check the proof.
        //
        // To generate P = <a,G> + <b,H'> + <a,b> Q, compute
        //             P = <a,G> + <b',H> + <a,b> Q,
        // where b' = b \circ y^(-n)
        let b_prime = b.iter().zip(util::exp_iter(y_inv)).map(|(bi, yi)| bi * yi);
        // a.iter() has Item=&Scalar, need Item=Scalar to chain with b_prime
        let a_prime = a.iter().cloned();

        let P = RistrettoPoint::vartime_multiscalar_mul(
            a_prime.chain(b_prime).chain(iter::once(c)),
            G.iter().chain(H.iter()).chain(iter::once(&Q)),
        );

        let mut prover_transcript = Transcript::new(b"innerproducttest");
        let mut verifier_transcript = Transcript::new(b"innerproducttest");

        let proof = InnerProductProof::new(
            &Q,
            &G_factors,
            &H_factors,
            G.clone(),
            H.clone(),
            a.clone(),
            b.clone(),
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                n,
                iter::repeat(Scalar::one()).take(n),
                util::exp_iter(y_inv).take(n),
                &P,
                &Q,
                &G,
                &H,
                &mut verifier_transcript,
            )
            .is_ok());

        let proof = InnerProductProof::from_bytes(proof.to_bytes().as_slice()).unwrap();
        let mut verifier_transcript = Transcript::new(b"innerproducttest");
        assert!(proof
            .verify(
                n,
                iter::repeat(Scalar::one()).take(n),
                util::exp_iter(y_inv).take(n),
                &P,
                &Q,
                &G,
                &H,
                &mut verifier_transcript,
            )
            .is_ok());
    }
}
