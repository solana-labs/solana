//! The equality sigma proof system.
//!
//! An equality proof is defined with respect to two cryptographic objects: a twisted ElGamal
//! ciphertext and a Pedersen commitment. The proof certifies that a given ciphertext and
//! commitment pair encrypts/encodes the same message. To generate the proof, a prover must provide
//! the decryption key for the ciphertext and the Pedersen opening for the commitment.
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
pub struct EqualityProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    z_s: Scalar,
    z_x: Scalar,
    z_r: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl EqualityProof {
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
        elgamal_keypair: &ElGamalKeypair,
        ciphertext: &ElGamalCiphertext,
        amount: u64,
        opening: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        // extract the relevant scalar and Ristretto points from the inputs
        let P_EG = elgamal_keypair.public.get_point();
        let D_EG = ciphertext.handle.get_point();

        let s = elgamal_keypair.secret.get_scalar();
        let x = Scalar::from(amount);
        let r = opening.get_scalar();

        // generate random masking factors that also serves as nonces
        let mut y_s = Scalar::random(&mut OsRng);
        let mut y_x = Scalar::random(&mut OsRng);
        let mut y_r = Scalar::random(&mut OsRng);

        let Y_0 = (&y_s * P_EG).compress();
        let Y_1 = RistrettoPoint::multiscalar_mul(vec![&y_x, &y_s], vec![&(*G), D_EG]).compress();
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

        EqualityProof {
            Y_0,
            Y_1,
            Y_2,
            z_s,
            z_x,
            z_r,
        }
    }

    /// Equality proof verifier.
    ///
    /// * `elgamal_pubkey` - The ElGamal pubkey associated with the ciphertext to be proved
    /// * `ciphertext` - The main ElGamal ciphertext to be proved
    /// * `commitment` - The main Pedersen commitment to be proved
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        elgamal_pubkey: &ElGamalPubkey,
        ciphertext: &ElGamalCiphertext,
        commitment: &PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Result<(), EqualityProofError> {
        // extract the relevant scalar and Ristretto points from the inputs
        let P_EG = elgamal_pubkey.get_point();
        let C_EG = ciphertext.commitment.get_point();
        let D_EG = ciphertext.handle.get_point();
        let C_Ped = commitment.get_point();

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
                P_EG,  // P_EG
                &(*H), // H
                &Y_0,  // Y_0
                &(*G), // G
                D_EG,  // D_EG
                C_EG,  // C_EG
                &Y_1,  // Y_1
                &(*G), // G
                &(*H), // H
                C_Ped, // C_Ped
                &Y_2,  // Y_2
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

        Ok(EqualityProof {
            Y_0,
            Y_1,
            Y_2,
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
    fn test_equality_proof_correctness() {
        // success case
        let elgamal_keypair = ElGamalKeypair::new_rand();
        let message: u64 = 55;

        let ciphertext = elgamal_keypair.public.encrypt(message);
        let (commitment, opening) = Pedersen::new(message);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = EqualityProof::new(
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

        // fail case: encrypted and committed messages are different
        let elgamal_keypair = ElGamalKeypair::new_rand();
        let encrypted_message: u64 = 55;
        let committed_message: u64 = 77;

        let ciphertext = elgamal_keypair.public.encrypt(encrypted_message);
        let (commitment, opening) = Pedersen::new(committed_message);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = EqualityProof::new(
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
    }

    #[test]
    fn test_equality_proof_edge_cases() {
        // if ElGamal public key zero (public key is invalid), then the proof should always reject
        let public = ElGamalPubkey::from_bytes(&[0u8; 32]).unwrap();
        let secret = ElGamalSecretKey::new_rand();

        let elgamal_keypair = ElGamalKeypair { public, secret };

        let message: u64 = 55;
        let ciphertext = elgamal_keypair.public.encrypt(message);
        let (commitment, opening) = Pedersen::new(message);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = EqualityProof::new(
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

        let proof = EqualityProof::new(
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

        let proof = EqualityProof::new(
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

        let proof = EqualityProof::new(
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
}
