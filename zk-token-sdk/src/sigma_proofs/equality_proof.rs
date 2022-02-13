//! The equality sigma proof system.
//!
//! An equality proof is defined with respect to two cryptographic objects: a twisted ElGamal
//! ciphertext and a Pedersen commitment. The proof certifies that a given ciphertext and
//! commitment pair encrypts/encodes the same message. To generate the proof, a prover must provide
//! the decryption key for the ciphertext and the Pedersen opening for the commitment.
//!
//! TODO: verify with respect to ciphertext
//!
//! The protocol guarantees computationally soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.

#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::{
        elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
        pedersen::{PedersenCommitment, PedersenOpening, G, H},
    },
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
    zeroize::Zeroize,
};
use {
    crate::{sigma_proofs::errors::EqualityProofError, transcript::TranscriptProtocol},
    arrayref::{array_ref, array_refs},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

/// Equality proof.
///
/// Contains all the elliptic curve and scalar components that make up the sigma protocol.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct CtxtCommEqualityProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    z_s: Scalar,
    z_x: Scalar,
    z_r: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl CtxtCommEqualityProof {
    /// Equality proof constructor.
    ///
    /// The function does *not* hash the public key, ciphertext, or commitment into the transcript.
    /// For security, the caller (the main protocol) should hash these public components prior to
    /// invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    ///
    /// Note that the proof constructor does not take the actual Pedersen commitment as input; it
    /// takes the associated Pedersen opening instead.
    ///
    /// * `elgamal_keypair` - The ElGamal keypair associated with the ciphertext to be proved
    /// * `ciphertext` - The main ElGamal ciphertext to be proved
    /// * `amount` - The message associated with the ElGamal ciphertext and Pedersen commitment
    /// * `opening` - The opening associated with the main Pedersen commitment to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new(
        keypair_source: &ElGamalKeypair,
        ciphertext_source: &ElGamalCiphertext,
        amount: u64,
        opening: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.equality_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the inputs
        let P_source = keypair_source.public.get_point();
        let D_source = ciphertext_source.handle.get_point();

        let s = keypair_source.secret.get_scalar();
        let x = Scalar::from(amount);
        let r = opening.get_scalar();

        // generate random masking factors that also serves as nonces
        let mut y_s = Scalar::random(&mut OsRng);
        let mut y_x = Scalar::random(&mut OsRng);
        let mut y_r = Scalar::random(&mut OsRng);

        let Y_0 = (&y_s * P_source).compress();
        let Y_1 =
            RistrettoPoint::multiscalar_mul(vec![&y_x, &y_s], vec![&(*G), D_source]).compress();
        let Y_2 = RistrettoPoint::multiscalar_mul(vec![&y_x, &y_r], vec![&(*G), &(*H)]).compress();

        // record masking factors in the transcript
        transcript.append_point(b"Y_0", &Y_0);
        transcript.append_point(b"Y_1", &Y_1);
        transcript.append_point(b"Y_2", &Y_2);

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

        CtxtCommEqualityProof {
            Y_0,
            Y_1,
            Y_2,
            z_s,
            z_x,
            z_r,
        }
    }

    /// Equality proof verifier. TODO: wrt commitment
    ///
    /// * `elgamal_pubkey` - The ElGamal pubkey associated with the ciphertext to be proved
    /// * `ciphertext` - The main ElGamal ciphertext to be proved
    /// * `commitment` - The main Pedersen commitment to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        pubkey_source: &ElGamalPubkey,
        ciphertext_source: &ElGamalCiphertext,
        commitment_dest: &PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Result<(), EqualityProofError> {
        transcript.equality_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the inputs
        let P_source = pubkey_source.get_point();
        let C_source = ciphertext_source.commitment.get_point();
        let D_source = ciphertext_source.handle.get_point();
        let C_dest = commitment_dest.get_point();

        // include Y_0, Y_1, Y_2 to transcript and extract challenges
        transcript.validate_and_append_point(b"Y_0", &self.Y_0)?;
        transcript.validate_and_append_point(b"Y_1", &self.Y_1)?;
        transcript.validate_and_append_point(b"Y_2", &self.Y_2)?;

        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w"); // w used for batch verification
        let ww = &w * &w;

        let w_negated = -&w;
        let ww_negated = -&ww;

        // check that the required algebraic condition holds
        let Y_0 = self.Y_0.decompress().ok_or(EqualityProofError::Format)?;
        let Y_1 = self.Y_1.decompress().ok_or(EqualityProofError::Format)?;
        let Y_2 = self.Y_2.decompress().ok_or(EqualityProofError::Format)?;

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![
                &self.z_s,           // z_s
                &(-&c),              // -c
                &(-&Scalar::one()),  // -identity
                &(&w * &self.z_x),   // w * z_x
                &(&w * &self.z_s),   // w * z_s
                &(&w_negated * &c),  // -w * c
                &w_negated,          // -w
                &(&ww * &self.z_x),  // ww * z_x
                &(&ww * &self.z_r),  // ww * z_r
                &(&ww_negated * &c), // -ww * c
                &ww_negated,         // -ww
            ],
            vec![
                P_source, // P_source
                &(*H),    // H
                &Y_0,     // Y_0
                &(*G),    // G
                D_source, // D_source
                C_source, // C_source
                &Y_1,     // Y_1
                &(*G),    // G
                &(*H),    // H
                C_dest,   // C_dest
                &Y_2,     // Y_2
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(EqualityProofError::AlgebraicRelation)
        }
    }

    pub fn to_bytes(&self) -> [u8; 192] {
        let mut buf = [0_u8; 192];
        buf[..32].copy_from_slice(self.Y_0.as_bytes());
        buf[32..64].copy_from_slice(self.Y_1.as_bytes());
        buf[64..96].copy_from_slice(self.Y_2.as_bytes());
        buf[96..128].copy_from_slice(self.z_s.as_bytes());
        buf[128..160].copy_from_slice(self.z_x.as_bytes());
        buf[160..192].copy_from_slice(self.z_r.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EqualityProofError> {
        let bytes = array_ref![bytes, 0, 192];
        let (Y_0, Y_1, Y_2, z_s, z_x, z_r) = array_refs![bytes, 32, 32, 32, 32, 32, 32];

        let Y_0 = CompressedRistretto::from_slice(Y_0);
        let Y_1 = CompressedRistretto::from_slice(Y_1);
        let Y_2 = CompressedRistretto::from_slice(Y_2);

        let z_s = Scalar::from_canonical_bytes(*z_s).ok_or(EqualityProofError::Format)?;
        let z_x = Scalar::from_canonical_bytes(*z_x).ok_or(EqualityProofError::Format)?;
        let z_r = Scalar::from_canonical_bytes(*z_r).ok_or(EqualityProofError::Format)?;

        Ok(CtxtCommEqualityProof {
            Y_0,
            Y_1,
            Y_2,
            z_s,
            z_x,
            z_r,
        })
    }
}

/// Equality proof.
///
/// Contains all the elliptic curve and scalar components that make up the sigma protocol.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct CtxtCtxtEqualityProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    Y_3: CompressedRistretto,
    z_s: Scalar,
    z_x: Scalar,
    z_r: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl CtxtCtxtEqualityProof {
    /// Equality proof constructor.
    ///
    /// The function does *not* hash the public key, ciphertext, or commitment into the transcript.
    /// For security, the caller (the main protocol) should hash these public components prior to
    /// invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    ///
    /// Note that the proof constructor does not take the actual Pedersen commitment as input; it
    /// takes the associated Pedersen opening instead.
    ///
    /// * `elgamal_keypair` - The ElGamal keypair associated with the ciphertext to be proved
    /// * `ciphertext` - The main ElGamal ciphertext to be proved
    /// * `amount` - The message associated with the ElGamal ciphertext and Pedersen commitment
    /// * `opening` - The opening associated with the main Pedersen commitment to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new(
        keypair_source: &ElGamalKeypair,
        pubkey_dest: &ElGamalPubkey,
        ciphertext_source: &ElGamalCiphertext,
        amount: u64,
        opening_dest: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.equality_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the inputs
        let P_source = keypair_source.public.get_point();
        let D_source = ciphertext_source.handle.get_point();
        let P_dest = pubkey_dest.get_point();

        let s = keypair_source.secret.get_scalar();
        let x = Scalar::from(amount);
        let r = opening_dest.get_scalar();

        // generate random masking factors that also serves as nonces
        let mut y_s = Scalar::random(&mut OsRng);
        let mut y_x = Scalar::random(&mut OsRng);
        let mut y_r = Scalar::random(&mut OsRng);

        let Y_0 = (&y_s * P_source).compress();
        let Y_1 =
            RistrettoPoint::multiscalar_mul(vec![&y_x, &y_s], vec![&(*G), D_source]).compress();
        let Y_2 = RistrettoPoint::multiscalar_mul(vec![&y_x, &y_r], vec![&(*G), &(*H)]).compress();
        let Y_3 = (&y_r * P_dest).compress();

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

        CtxtCtxtEqualityProof {
            Y_0,
            Y_1,
            Y_2,
            Y_3,
            z_s,
            z_x,
            z_r,
        }
    }

    /// Equality proof verifier. TODO: wrt commitment
    ///
    /// * `elgamal_pubkey` - The ElGamal pubkey associated with the ciphertext to be proved
    /// * `ciphertext` - The main ElGamal ciphertext to be proved
    /// * `commitment` - The main Pedersen commitment to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        pubkey_source: &ElGamalPubkey,
        pubkey_dest: &ElGamalPubkey,
        ciphertext_source: &ElGamalCiphertext,
        ciphertext_dest: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), EqualityProofError> {
        transcript.equality_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the inputs
        let P_source = pubkey_source.get_point();
        let C_source = ciphertext_source.commitment.get_point();
        let D_source = ciphertext_source.handle.get_point();

        let P_dest = pubkey_dest.get_point();
        let C_dest = ciphertext_dest.commitment.get_point();
        let D_dest = ciphertext_dest.handle.get_point();

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
        let Y_0 = self.Y_0.decompress().ok_or(EqualityProofError::Format)?;
        let Y_1 = self.Y_1.decompress().ok_or(EqualityProofError::Format)?;
        let Y_2 = self.Y_2.decompress().ok_or(EqualityProofError::Format)?;
        let Y_3 = self.Y_3.decompress().ok_or(EqualityProofError::Format)?;

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
                P_source, // P_source
                &(*H),    // H
                &Y_0,     // Y_0
                &(*G),    // G
                D_source, // D_source
                C_source, // C_source
                &Y_1,     // Y_1
                &(*G),    // G
                &(*H),    // H
                C_dest,   // C_dest
                &Y_2,     // Y_2
                P_dest,   // P_dest
                D_dest,   // D_dest
                &Y_3,     // Y_3
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(EqualityProofError::AlgebraicRelation)
        }
    }

    pub fn to_bytes(&self) -> [u8; 224] {
        let mut buf = [0_u8; 224];
        buf[..32].copy_from_slice(self.Y_0.as_bytes());
        buf[32..64].copy_from_slice(self.Y_1.as_bytes());
        buf[64..96].copy_from_slice(self.Y_2.as_bytes());
        buf[96..128].copy_from_slice(self.Y_3.as_bytes());
        buf[128..160].copy_from_slice(self.z_s.as_bytes());
        buf[160..192].copy_from_slice(self.z_x.as_bytes());
        buf[192..224].copy_from_slice(self.z_r.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EqualityProofError> {
        let bytes = array_ref![bytes, 0, 224];
        let (Y_0, Y_1, Y_2, Y_3, z_s, z_x, z_r) = array_refs![bytes, 32, 32, 32, 32, 32, 32, 32];

        let Y_0 = CompressedRistretto::from_slice(Y_0);
        let Y_1 = CompressedRistretto::from_slice(Y_1);
        let Y_2 = CompressedRistretto::from_slice(Y_2);
        let Y_3 = CompressedRistretto::from_slice(Y_3);

        let z_s = Scalar::from_canonical_bytes(*z_s).ok_or(EqualityProofError::Format)?;
        let z_x = Scalar::from_canonical_bytes(*z_x).ok_or(EqualityProofError::Format)?;
        let z_r = Scalar::from_canonical_bytes(*z_r).ok_or(EqualityProofError::Format)?;

        Ok(CtxtCtxtEqualityProof {
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
    use crate::encryption::{elgamal::ElGamalSecretKey, pedersen::Pedersen};

    #[test]
    fn test_ciphertext_commitment_equality_proof_correctness() {
        // success case
        let keypair_source = ElGamalKeypair::new_rand();
        let message: u64 = 55;

        let ciphertext_source = keypair_source.public.encrypt(message);
        let (commitment_dest, opening_dest) = Pedersen::new(message);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCommEqualityProof::new(
            &keypair_source,
            &ciphertext_source,
            message,
            &opening_dest,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &keypair_source.public,
                &ciphertext_source,
                &commitment_dest,
                &mut transcript_verifier
            )
            .is_ok());

        // fail case: encrypted and committed messages are different
        let keypair_source = ElGamalKeypair::new_rand();
        let encrypted_message: u64 = 55;
        let committed_message: u64 = 77;

        let ciphertext_source = keypair_source.public.encrypt(encrypted_message);
        let (commitment_dest, opening_dest) = Pedersen::new(committed_message);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCommEqualityProof::new(
            &keypair_source,
            &ciphertext_source,
            message,
            &opening_dest,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &keypair_source.public,
                &ciphertext_source,
                &commitment_dest,
                &mut transcript_verifier
            )
            .is_err());
    }

    #[test]
    fn test_ciphertext_commitment_equality_proof_edge_cases() {
        // if ElGamal public key zero (public key is invalid), then the proof should always reject
        let public = ElGamalPubkey::from_bytes(&[0u8; 32]).unwrap();
        let secret = ElGamalSecretKey::new_rand();

        let elgamal_keypair = ElGamalKeypair { public, secret };

        let message: u64 = 55;
        let ciphertext = elgamal_keypair.public.encrypt(message);
        let (commitment, opening) = Pedersen::new(message);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCommEqualityProof::new(
            &elgamal_keypair,
            &ciphertext,
            message,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &elgamal_keypair.public,
                &ciphertext,
                &commitment,
                &mut transcript_verifier
            )
            .is_err());

        // if ciphertext is all-zero (valid commitment of 0) and commitment is also all-zero, then
        // the proof should still accept
        let elgamal_keypair = ElGamalKeypair::new_rand();

        let message: u64 = 0;
        let ciphertext = ElGamalCiphertext::from_bytes(&[0u8; 64]).unwrap();
        let commitment = PedersenCommitment::from_bytes(&[0u8; 32]).unwrap();
        let opening = PedersenOpening::from_bytes(&[0u8; 32]).unwrap();

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCommEqualityProof::new(
            &elgamal_keypair,
            &ciphertext,
            message,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &elgamal_keypair.public,
                &ciphertext,
                &commitment,
                &mut transcript_verifier
            )
            .is_ok());

        // if commitment is all-zero and the ciphertext is a correct encryption of 0, then the
        // proof should still accept
        let elgamal_keypair = ElGamalKeypair::new_rand();

        let message: u64 = 0;
        let ciphertext = elgamal_keypair.public.encrypt(message);
        let commitment = PedersenCommitment::from_bytes(&[0u8; 32]).unwrap();
        let opening = PedersenOpening::from_bytes(&[0u8; 32]).unwrap();

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCommEqualityProof::new(
            &elgamal_keypair,
            &ciphertext,
            message,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &elgamal_keypair.public,
                &ciphertext,
                &commitment,
                &mut transcript_verifier
            )
            .is_ok());

        // if ciphertext is all zero and commitment correctly encodes 0, then the proof should
        // still accept
        let elgamal_keypair = ElGamalKeypair::new_rand();

        let message: u64 = 0;
        let ciphertext = ElGamalCiphertext::from_bytes(&[0u8; 64]).unwrap();
        let (commitment, opening) = Pedersen::new(message);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCommEqualityProof::new(
            &elgamal_keypair,
            &ciphertext,
            message,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &elgamal_keypair.public,
                &ciphertext,
                &commitment,
                &mut transcript_verifier
            )
            .is_ok());
    }

    #[test]
    fn test_ciphertext_ciphertext_equality_proof_correctness() {
        // success case
        let keypair_source = ElGamalKeypair::new_rand();
        let keypair_dest = ElGamalKeypair::new_rand();
        let message: u64 = 55;

        let ciphertext_source = keypair_source.public.encrypt(message);

        let opening_dest = PedersenOpening::new_rand();
        let ciphertext_dest = keypair_dest.public.encrypt_with(message, &opening_dest);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCtxtEqualityProof::new(
            &keypair_source,
            &keypair_dest.public,
            &ciphertext_source,
            message,
            &opening_dest,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &keypair_source.public,
                &keypair_dest.public,
                &ciphertext_source,
                &ciphertext_dest,
                &mut transcript_verifier
            )
            .is_ok());

        // fail case: encrypted and committed messages are different
        let message_source: u64 = 55;
        let message_dest: u64 = 77;

        let ciphertext_source = keypair_source.public.encrypt(message_source);

        let opening_dest = PedersenOpening::new_rand();
        let ciphertext_dest = keypair_dest
            .public
            .encrypt_with(message_dest, &opening_dest);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = CtxtCtxtEqualityProof::new(
            &keypair_source,
            &keypair_dest.public,
            &ciphertext_source,
            message,
            &opening_dest,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &keypair_source.public,
                &keypair_dest.public,
                &ciphertext_source,
                &ciphertext_dest,
                &mut transcript_verifier
            )
            .is_err());
    }
}
