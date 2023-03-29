//! The zero-balance sigma proof system.
//!
//! A zero-balance proof is defined with respect to a twisted ElGamal ciphertext. The proof
//! certifies that a given ciphertext encrypts the message 0 (`Scalar::zero()`). To generate the
//! proof, a prover must provide the decryption key for the ciphertext.
//!
//! The protocol guarantees computationally soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::H,
        },
        errors::ProofVerificationError,
    },
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
    zeroize::Zeroize,
};
use {
    crate::{sigma_proofs::errors::ZeroBalanceProofError, transcript::TranscriptProtocol},
    arrayref::{array_ref, array_refs},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::IsIdentity,
    },
    merlin::Transcript,
};

/// Zero-balance proof.
///
/// Contains all the elliptic curve and scalar components that make up the sigma protocol.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct ZeroBalanceProof {
    Y_P: CompressedRistretto,
    Y_D: CompressedRistretto,
    z: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl ZeroBalanceProof {
    /// Zero-balance proof constructor.
    ///
    /// The function does *not* hash the public key and ciphertext into the transcript. For
    /// security, the caller (the main protocol) should hash these public components prior to
    /// invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    ///
    /// Note that the proof constructor does not take the actual ElGamal ciphertext as input; it
    /// uses the ElGamal private key instead to generate the proof.
    ///
    /// * `elgamal_keypair` - The ElGamal keypair associated with the ciphertext to be proved
    /// * `ciphertext` - The main ElGamal ciphertext to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new(
        elgamal_keypair: &ElGamalKeypair,
        ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.zero_balance_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let P = elgamal_keypair.public.get_point();
        let s = elgamal_keypair.secret.get_scalar();
        let D = ciphertext.handle.get_point();

        // generate a random masking factor that also serves as a nonce
        let mut y = Scalar::random(&mut OsRng);
        let Y_P = (&y * P).compress();
        let Y_D = (&y * D).compress();

        // record Y in the transcript and receive a challenge scalar
        transcript.append_point(b"Y_P", &Y_P);
        transcript.append_point(b"Y_D", &Y_D);

        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // compute the masked secret key
        let z = &(&c * s) + &y;

        // zeroize random scalar
        y.zeroize();

        Self { Y_P, Y_D, z }
    }

    /// Zero-balance proof verifier.
    ///
    /// * `elgamal_pubkey` - The ElGamal pubkey associated with the ciphertext to be proved
    /// * `ciphertext` - The main ElGamal ciphertext to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        elgamal_pubkey: &ElGamalPubkey,
        ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ZeroBalanceProofError> {
        transcript.zero_balance_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the input
        let P = elgamal_pubkey.get_point();
        let C = ciphertext.commitment.get_point();
        let D = ciphertext.handle.get_point();

        // record Y in transcript and receive challenge scalars
        transcript.validate_and_append_point(b"Y_P", &self.Y_P)?;
        transcript.append_point(b"Y_D", &self.Y_D);

        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w"); // w used for batch verification

        let w_negated = -&w;

        // decompress Y or return verification error
        let Y_P = self
            .Y_P
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;
        let Y_D = self
            .Y_D
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;

        // check the required algebraic relation
        let check = RistrettoPoint::multiscalar_mul(
            vec![
                &self.z,            // z
                &(-&c),             // -c
                &(-&Scalar::one()), // -identity
                &(&w * &self.z),    // w * z
                &(&w_negated * &c), // -w * c
                &w_negated,         // -w
            ],
            vec![
                P,     // P
                &(*H), // H
                &Y_P,  // Y_P
                D,     // D
                C,     // C
                &Y_D,  // Y_D
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ProofVerificationError::AlgebraicRelation.into())
        }
    }

    pub fn to_bytes(&self) -> [u8; 96] {
        let mut buf = [0_u8; 96];
        buf[..32].copy_from_slice(self.Y_P.as_bytes());
        buf[32..64].copy_from_slice(self.Y_D.as_bytes());
        buf[64..96].copy_from_slice(self.z.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ZeroBalanceProofError> {
        if bytes.len() != 96 {
            return Err(ProofVerificationError::Deserialization.into());
        }

        let bytes = array_ref![bytes, 0, 96];
        let (Y_P, Y_D, z) = array_refs![bytes, 32, 32, 32];

        let Y_P = CompressedRistretto::from_slice(Y_P);
        let Y_D = CompressedRistretto::from_slice(Y_D);

        let z = Scalar::from_canonical_bytes(*z).ok_or(ProofVerificationError::Deserialization)?;

        Ok(ZeroBalanceProof { Y_P, Y_D, z })
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{
            elgamal::{DecryptHandle, ElGamalKeypair, ElGamalSecretKey},
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
    };

    #[test]
    fn test_zero_balance_proof_correctness() {
        let source_keypair = ElGamalKeypair::new_rand();

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        // general case: encryption of 0
        let elgamal_ciphertext = source_keypair.public.encrypt(0_u64);
        let proof =
            ZeroBalanceProof::new(&source_keypair, &elgamal_ciphertext, &mut prover_transcript);
        assert!(proof
            .verify(
                &source_keypair.public,
                &elgamal_ciphertext,
                &mut verifier_transcript
            )
            .is_ok());

        // general case: encryption of > 0
        let elgamal_ciphertext = source_keypair.public.encrypt(1_u64);
        let proof =
            ZeroBalanceProof::new(&source_keypair, &elgamal_ciphertext, &mut prover_transcript);
        assert!(proof
            .verify(
                &source_keypair.public,
                &elgamal_ciphertext,
                &mut verifier_transcript
            )
            .is_err());
    }

    #[test]
    fn test_zero_balance_proof_edge_cases() {
        let source_keypair = ElGamalKeypair::new_rand();

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        // all zero ciphertext should always be a valid encryption of 0
        let ciphertext = ElGamalCiphertext::from_bytes(&[0u8; 64]).unwrap();

        let proof = ZeroBalanceProof::new(&source_keypair, &ciphertext, &mut prover_transcript);

        assert!(proof
            .verify(
                &source_keypair.public,
                &ciphertext,
                &mut verifier_transcript
            )
            .is_ok());

        // if only either commitment or handle is zero, the ciphertext is always invalid and proof
        // verification should always reject
        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let zeroed_commitment = PedersenCommitment::from_bytes(&[0u8; 32]).unwrap();
        let handle = source_keypair
            .public
            .decrypt_handle(&PedersenOpening::new_rand());

        let ciphertext = ElGamalCiphertext {
            commitment: zeroed_commitment,
            handle,
        };

        let proof = ZeroBalanceProof::new(&source_keypair, &ciphertext, &mut prover_transcript);

        assert!(proof
            .verify(
                &source_keypair.public,
                &ciphertext,
                &mut verifier_transcript
            )
            .is_err());

        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let (zeroed_commitment, _) = Pedersen::new(0_u64);
        let ciphertext = ElGamalCiphertext {
            commitment: zeroed_commitment,
            handle: DecryptHandle::from_bytes(&[0u8; 32]).unwrap(),
        };

        let proof = ZeroBalanceProof::new(&source_keypair, &ciphertext, &mut prover_transcript);

        assert!(proof
            .verify(
                &source_keypair.public,
                &ciphertext,
                &mut verifier_transcript
            )
            .is_err());

        // if public key is always zero, then the proof should always reject
        let mut prover_transcript = Transcript::new(b"test");
        let mut verifier_transcript = Transcript::new(b"test");

        let public = ElGamalPubkey::from_bytes(&[0u8; 32]).unwrap();
        let secret = ElGamalSecretKey::new_rand();

        let elgamal_keypair = ElGamalKeypair { public, secret };

        let ciphertext = elgamal_keypair.public.encrypt(0_u64);

        let proof = ZeroBalanceProof::new(&source_keypair, &ciphertext, &mut prover_transcript);

        assert!(proof
            .verify(
                &source_keypair.public,
                &ciphertext,
                &mut verifier_transcript
            )
            .is_err());
    }
}
