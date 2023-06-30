//! The ciphertext-ciphertext equality sigma proof system.
//!
//! The protocol guarantees computational soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pedersen::{PedersenOpening, G, H},
        },
        errors::ProofVerificationError,
        sigma_proofs::{canonical_scalar_from_optional_slice, ristretto_point_from_optional_slice},
        UNIT_LEN,
    },
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
    zeroize::Zeroize,
};
use {
    crate::{sigma_proofs::errors::EqualityProofError, transcript::TranscriptProtocol},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

/// Byte length of a ciphertext-ciphertext equality proof.
const CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN: usize = UNIT_LEN * 7;

/// The ciphertext-ciphertext equality proof.
///
/// Contains all the elliptic curve and scalar components that make up the sigma protocol.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct CiphertextCiphertextEqualityProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    Y_3: CompressedRistretto,
    z_s: Scalar,
    z_x: Scalar,
    z_r: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl CiphertextCiphertextEqualityProof {
    /// Creates a ciphertext-ciphertext equality proof.
    ///
    /// The function does *not* hash the public key, ciphertext, or commitment into the transcript.
    /// For security, the caller (the main protocol) should hash these public components prior to
    /// invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    ///
    /// * `source_keypair` - The ElGamal keypair associated with the first ciphertext to be proved
    /// * `destination_pubkey` - The ElGamal pubkey associated with the second ElGamal ciphertext
    /// * `source_ciphertext` - The first ElGamal ciphertext for which the prover knows a
    /// decryption key for
    /// * `destination_opening` - The opening (randomness) associated with the second ElGamal ciphertext
    /// * `amount` - The message associated with the ElGamal ciphertext and Pedersen commitment
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new(
        source_keypair: &ElGamalKeypair,
        destination_pubkey: &ElGamalPubkey,
        source_ciphertext: &ElGamalCiphertext,
        destination_opening: &PedersenOpening,
        amount: u64,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.equality_proof_domain_separator();

        // extract the relevant scalar and Ristretto points from the inputs
        let P_source = source_keypair.pubkey().get_point();
        let D_source = source_ciphertext.handle.get_point();
        let P_destination = destination_pubkey.get_point();

        let s = source_keypair.secret().get_scalar();
        let x = Scalar::from(amount);
        let r = destination_opening.get_scalar();

        // generate random masking factors that also serves as nonces
        let mut y_s = Scalar::random(&mut OsRng);
        let mut y_x = Scalar::random(&mut OsRng);
        let mut y_r = Scalar::random(&mut OsRng);

        let Y_0 = (&y_s * P_source).compress();
        let Y_1 =
            RistrettoPoint::multiscalar_mul(vec![&y_x, &y_s], vec![&(*G), D_source]).compress();
        let Y_2 = RistrettoPoint::multiscalar_mul(vec![&y_x, &y_r], vec![&(*G), &(*H)]).compress();
        let Y_3 = (&y_r * P_destination).compress();

        // record masking factors in the transcript
        transcript.append_point(b"Y_0", &Y_0);
        transcript.append_point(b"Y_1", &Y_1);
        transcript.append_point(b"Y_2", &Y_2);
        transcript.append_point(b"Y_3", &Y_3);

        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // compute the masked values
        let z_s = &(&c * s) + &y_s;
        let z_x = &(&c * &x) + &y_x;
        let z_r = &(&c * r) + &y_r;

        // zeroize random scalars
        y_s.zeroize();
        y_x.zeroize();
        y_r.zeroize();

        CiphertextCiphertextEqualityProof {
            Y_0,
            Y_1,
            Y_2,
            Y_3,
            z_s,
            z_x,
            z_r,
        }
    }

    /// Verifies a ciphertext-ciphertext equality proof.
    ///
    /// * `source_pubkey` - The ElGamal pubkey associated with the first ciphertext to be proved
    /// * `destination_pubkey` - The ElGamal pubkey associated with the second ciphertext to be proved
    /// * `source_ciphertext` - The first ElGamal ciphertext to be proved
    /// * `destination_ciphertext` - The second ElGamal ciphertext to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        source_ciphertext: &ElGamalCiphertext,
        destination_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), EqualityProofError> {
        transcript.equality_proof_domain_separator();

        // extract the relevant scalar and Ristretto points from the inputs
        let P_source = source_pubkey.get_point();
        let C_source = source_ciphertext.commitment.get_point();
        let D_source = source_ciphertext.handle.get_point();

        let P_destination = destination_pubkey.get_point();
        let C_destination = destination_ciphertext.commitment.get_point();
        let D_destination = destination_ciphertext.handle.get_point();

        // include Y_0, Y_1, Y_2 to transcript and extract challenges
        transcript.validate_and_append_point(b"Y_0", &self.Y_0)?;
        transcript.validate_and_append_point(b"Y_1", &self.Y_1)?;
        transcript.validate_and_append_point(b"Y_2", &self.Y_2)?;
        transcript.validate_and_append_point(b"Y_3", &self.Y_3)?;

        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w"); // w used for batch verification
        let ww = &w * &w;
        let www = &w * &ww;

        let w_negated = -&w;
        let ww_negated = -&ww;
        let www_negated = -&www;

        // check that the required algebraic condition holds
        let Y_0 = self
            .Y_0
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;
        let Y_1 = self
            .Y_1
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;
        let Y_2 = self
            .Y_2
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;
        let Y_3 = self
            .Y_3
            .decompress()
            .ok_or(ProofVerificationError::Deserialization)?;

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![
                &self.z_s,            // z_s
                &(-&c),               // -c
                &(-&Scalar::one()),   // -identity
                &(&w * &self.z_x),    // w * z_x
                &(&w * &self.z_s),    // w * z_s
                &(&w_negated * &c),   // -w * c
                &w_negated,           // -w
                &(&ww * &self.z_x),   // ww * z_x
                &(&ww * &self.z_r),   // ww * z_r
                &(&ww_negated * &c),  // -ww * c
                &ww_negated,          // -ww
                &(&www * &self.z_r),  // z_r
                &(&www_negated * &c), // -www * c
                &www_negated,
            ],
            vec![
                P_source,      // P_source
                &(*H),         // H
                &Y_0,          // Y_0
                &(*G),         // G
                D_source,      // D_source
                C_source,      // C_source
                &Y_1,          // Y_1
                &(*G),         // G
                &(*H),         // H
                C_destination, // C_destination
                &Y_2,          // Y_2
                P_destination, // P_destination
                D_destination, // D_destination
                &Y_3,          // Y_3
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ProofVerificationError::AlgebraicRelation.into())
        }
    }

    pub fn to_bytes(&self) -> [u8; CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN] {
        let mut buf = [0_u8; CIPHERTEXT_CIPHERTEXT_EQUALITY_PROOF_LEN];
        let mut chunks = buf.chunks_mut(UNIT_LEN);

        chunks.next().unwrap().copy_from_slice(self.Y_0.as_bytes());
        chunks.next().unwrap().copy_from_slice(self.Y_1.as_bytes());
        chunks.next().unwrap().copy_from_slice(self.Y_2.as_bytes());
        chunks.next().unwrap().copy_from_slice(self.Y_3.as_bytes());
        chunks.next().unwrap().copy_from_slice(self.z_s.as_bytes());
        chunks.next().unwrap().copy_from_slice(self.z_x.as_bytes());
        chunks.next().unwrap().copy_from_slice(self.z_r.as_bytes());

        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EqualityProofError> {
        let mut chunks = bytes.chunks(UNIT_LEN);

        let Y_0 = ristretto_point_from_optional_slice(chunks.next())?;
        let Y_1 = ristretto_point_from_optional_slice(chunks.next())?;
        let Y_2 = ristretto_point_from_optional_slice(chunks.next())?;
        let Y_3 = ristretto_point_from_optional_slice(chunks.next())?;
        let z_s = canonical_scalar_from_optional_slice(chunks.next())?;
        let z_x = canonical_scalar_from_optional_slice(chunks.next())?;
        let z_r = canonical_scalar_from_optional_slice(chunks.next())?;

        Ok(CiphertextCiphertextEqualityProof {
            Y_0,
            Y_1,
            Y_2,
            Y_3,
            z_s,
            z_x,
            z_r,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ciphertext_ciphertext_equality_proof_correctness() {
        // success case
        let source_keypair = ElGamalKeypair::new_rand();
        let destination_keypair = ElGamalKeypair::new_rand();
        let message: u64 = 55;

        let source_ciphertext = source_keypair.pubkey().encrypt(message);

        let destination_opening = PedersenOpening::new_rand();
        let destination_ciphertext = destination_keypair
            .pubkey()
            .encrypt_with(message, &destination_opening);

        let mut prover_transcript = Transcript::new(b"Test");
        let mut verifier_transcript = Transcript::new(b"Test");

        let proof = CiphertextCiphertextEqualityProof::new(
            &source_keypair,
            destination_keypair.pubkey(),
            &source_ciphertext,
            &destination_opening,
            message,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                source_keypair.pubkey(),
                destination_keypair.pubkey(),
                &source_ciphertext,
                &destination_ciphertext,
                &mut verifier_transcript
            )
            .is_ok());

        // fail case: encrypted and committed messages are different
        let source_message: u64 = 55;
        let destination_message: u64 = 77;

        let source_ciphertext = source_keypair.pubkey().encrypt(source_message);

        let destination_opening = PedersenOpening::new_rand();
        let destination_ciphertext = destination_keypair
            .pubkey()
            .encrypt_with(destination_message, &destination_opening);

        let mut prover_transcript = Transcript::new(b"Test");
        let mut verifier_transcript = Transcript::new(b"Test");

        let proof = CiphertextCiphertextEqualityProof::new(
            &source_keypair,
            destination_keypair.pubkey(),
            &source_ciphertext,
            &destination_opening,
            message,
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                source_keypair.pubkey(),
                destination_keypair.pubkey(),
                &source_ciphertext,
                &destination_ciphertext,
                &mut verifier_transcript
            )
            .is_err());
    }
}
