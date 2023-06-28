//! The public-key (validity) proof system.
//!
//! The protocol guarantees computational soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalKeypair, ElGamalPubkey},
            pedersen::H,
        },
        sigma_proofs::{canonical_scalar_from_optional_slice, ristretto_point_from_optional_slice},
        UNIT_LEN,
    },
    rand::rngs::OsRng,
    zeroize::Zeroize,
};
use {
    crate::{
        errors::ProofVerificationError, sigma_proofs::errors::PubkeyValidityProofError,
        transcript::TranscriptProtocol,
    },
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

/// Byte length of a public key validity proof.
const PUBKEY_VALIDITY_PROOF_LEN: usize = UNIT_LEN * 2;

/// Public-key proof.
///
/// Contains all the elliptic curve and scalar components that make up the sigma protocol.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct PubkeyValidityProof {
    Y: CompressedRistretto,
    z: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl PubkeyValidityProof {
    /// Creates a public key validity proof.
    ///
    /// The function does *not* hash the public key and ciphertext into the transcript. For
    /// security, the caller (the main protocol) should hash these public key components prior to
    /// invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    ///
    /// This function panics if the provided keypair is not valid (i.e. secret key is not
    /// invertible).
    ///
    /// * `elgamal_keypair` = The ElGamal keypair that pertains to the ElGamal public key to be
    /// proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new(elgamal_keypair: &ElGamalKeypair, transcript: &mut Transcript) -> Self {
        transcript.pubkey_proof_domain_separator();

        // extract the relevant scalar and Ristretto points from the input
        let s = elgamal_keypair.secret().get_scalar();

        assert!(s != &Scalar::zero());
        let s_inv = s.invert();

        // generate a random masking factor that also serves as a nonce
        let mut y = Scalar::random(&mut OsRng);
        let Y = (&y * &(*H)).compress();

        // record masking factors in transcript and get challenges
        transcript.append_point(b"Y", &Y);
        let c = transcript.challenge_scalar(b"c");

        // compute masked secret key
        let z = &(&c * s_inv) + &y;

        y.zeroize();

        Self { Y, z }
    }

    /// Verifies a public key validity proof.
    ///
    /// * `elgamal_pubkey` - The ElGamal public key to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        elgamal_pubkey: &ElGamalPubkey,
        transcript: &mut Transcript,
    ) -> Result<(), PubkeyValidityProofError> {
        transcript.pubkey_proof_domain_separator();

        // extract the relvant scalar and Ristretto points from the input
        let P = elgamal_pubkey.get_point();

        // include Y to transcript and extract challenge
        transcript.validate_and_append_point(b"Y", &self.Y)?;
        let c = transcript.challenge_scalar(b"c");

        // check that the required algebraic condition holds
        let Y = self
            .Y
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![&self.z, &(-&c), &(-&Scalar::one())],
            vec![&(*H), P, &Y],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ProofVerificationError::AlgebraicRelation.into())
        }
    }

    pub fn to_bytes(&self) -> [u8; PUBKEY_VALIDITY_PROOF_LEN] {
        let mut buf = [0_u8; PUBKEY_VALIDITY_PROOF_LEN];
        let mut chunks = buf.chunks_mut(UNIT_LEN);
        chunks.next().unwrap().copy_from_slice(self.Y.as_bytes());
        chunks.next().unwrap().copy_from_slice(self.z.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PubkeyValidityProofError> {
        let mut chunks = bytes.chunks(UNIT_LEN);
        let Y = ristretto_point_from_optional_slice(chunks.next())?;
        let z = canonical_scalar_from_optional_slice(chunks.next())?;
        Ok(PubkeyValidityProof { Y, z })
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{pubkey::Pubkey, signature::Keypair},
    };

    #[test]
    fn test_pubkey_proof_correctness() {
        // random ElGamal keypair
        let keypair = ElGamalKeypair::new_rand();

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = PubkeyValidityProof::new(&keypair, &mut prover_transcript);
        assert!(proof
            .verify(keypair.pubkey(), &mut verifier_transcript)
            .is_ok());

        // derived ElGamal keypair
        let keypair =
            ElGamalKeypair::new_from_signer(&Keypair::new(), Pubkey::default().as_ref()).unwrap();

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let proof = PubkeyValidityProof::new(&keypair, &mut prover_transcript);
        assert!(proof
            .verify(keypair.pubkey(), &mut verifier_transcript)
            .is_ok());
    }
}
