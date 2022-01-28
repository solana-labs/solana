//! The ciphertext validity sigma proof system.
//!
//! The ciphertext validity proof is defined with respect to a Pedersen commitment and two
//! decryption handles. The proof certifies that a given Pedersen commitment can be decrypted using
//! ElGamal private keys that are associated with each of the two decryption handles. To generate
//! the proof, a prover must provide the Pedersen opening associated with the commitment.
//!
//! The protocol guarantees computational soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.

#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::{
        elgamal::{DecryptHandle, ElGamalPubkey},
        pedersen::{PedersenCommitment, PedersenOpening, G, H},
    },
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
    zeroize::Zeroize,
};
use {
    crate::{sigma_proofs::errors::ValidityProofError, transcript::TranscriptProtocol},
    arrayref::{array_ref, array_refs},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

/// The ciphertext validity proof.
///
/// Contains all the elliptic curve and scalar components that make up the sigma protocol.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct ValidityProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    z_r: Scalar,
    z_x: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl ValidityProof {
    /// The ciphertext validity proof constructor.
    ///
    /// The function does *not* hash the public keys, commitment, or decryption handles into the
    /// transcript. For security, the caller (the main protocol) should hash these public
    /// components prior to invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    ///
    /// Note that the proof constructor does not take the actual Pedersen commitment or decryption
    /// handles as input; it only takes the associated Pedersen opening instead.
    ///
    /// * `(pubkey_dest, pubkey_auditor)` - The ElGamal public keys associated with the decryption
    /// handles
    /// * `amount` - The committed message in the commitment
    /// * `opening` - The opening associated with the Pedersen commitment
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn new<T: Into<Scalar>>(
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        amount: T,
        opening: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        // extract the relevant scalar and Ristretto points from the inputs
        let P_dest = pubkey_dest.get_point();
        let P_auditor = pubkey_auditor.get_point();

        let x = amount.into();
        let r = opening.get_scalar();

        // generate random masking factors that also serves as nonces
        let mut y_r = Scalar::random(&mut OsRng);
        let mut y_x = Scalar::random(&mut OsRng);

        let Y_0 = RistrettoPoint::multiscalar_mul(vec![&y_r, &y_x], vec![&(*H), &(*G)]).compress();
        let Y_1 = (&y_r * P_dest).compress();
        let Y_2 = (&y_r * P_auditor).compress();

        // record masking factors in transcript and get challenges
        transcript.append_point(b"Y_0", &Y_0);
        transcript.append_point(b"Y_1", &Y_1);
        transcript.append_point(b"Y_2", &Y_2);

        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // compute masked message and opening
        let z_r = &(&c * r) + &y_r;
        let z_x = &(&c * &x) + &y_x;

        y_r.zeroize();
        y_x.zeroize();

        Self {
            Y_0,
            Y_1,
            Y_2,
            z_r,
            z_x,
        }
    }

    /// The ciphertext validity proof verifier.
    ///
    /// * `commitment` - The Pedersen commitment
    /// * `(pubkey_dest, pubkey_auditor)` - The ElGamal pubkeys associated with the decryption
    /// handles
    /// * `(handle_dest, handle_audtior)` - The decryption handles
    /// * `transcript` - The transcript that does the bookkeeping for the Fiat-Shamir heuristic
    pub fn verify(
        self,
        commitment: &PedersenCommitment,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        (handle_dest, handle_auditor): (&DecryptHandle, &DecryptHandle),
        transcript: &mut Transcript,
    ) -> Result<(), ValidityProofError> {
        // include Y_0, Y_1, Y_2 to transcript and extract challenges
        transcript.validate_and_append_point(b"Y_0", &self.Y_0)?;
        transcript.validate_and_append_point(b"Y_1", &self.Y_1)?;
        transcript.validate_and_append_point(b"Y_2", &self.Y_2)?;

        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w");
        let ww = &w * &w;

        let w_negated = -&w;
        let ww_negated = -&ww;

        // check the required algebraic conditions
        let Y_0 = self.Y_0.decompress().ok_or(ValidityProofError::Format)?;
        let Y_1 = self.Y_1.decompress().ok_or(ValidityProofError::Format)?;
        let Y_2 = self.Y_2.decompress().ok_or(ValidityProofError::Format)?;

        let P_dest = pubkey_dest.get_point();
        let P_auditor = pubkey_auditor.get_point();

        let C = commitment.get_point();
        let D_dest = handle_dest.get_point();
        let D_auditor = handle_auditor.get_point();

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![
                &self.z_r,           // z_r
                &self.z_x,           // z_x
                &(-&c),              // -c
                &-(&Scalar::one()),  // -identity
                &(&w * &self.z_r),   // w * z_r
                &(&w_negated * &c),  // -w * c
                &w_negated,          // -w
                &(&ww * &self.z_r),  // ww * z_r
                &(&ww_negated * &c), // -ww * c
                &ww_negated,         // -ww
            ],
            vec![
                &(*H),     // H
                &(*G),     // G
                C,         // C
                &Y_0,      // Y_0
                P_dest,    // P_dest
                D_dest,    // D_dest
                &Y_1,      // Y_1
                P_auditor, // P_auditor
                D_auditor, // D_auditor
                &Y_2,      // Y_2
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ValidityProofError::AlgebraicRelation)
        }
    }

    pub fn to_bytes(&self) -> [u8; 160] {
        let mut buf = [0_u8; 160];
        buf[..32].copy_from_slice(self.Y_0.as_bytes());
        buf[32..64].copy_from_slice(self.Y_1.as_bytes());
        buf[64..96].copy_from_slice(self.Y_2.as_bytes());
        buf[96..128].copy_from_slice(self.z_r.as_bytes());
        buf[128..160].copy_from_slice(self.z_x.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidityProofError> {
        let bytes = array_ref![bytes, 0, 160];
        let (Y_0, Y_1, Y_2, z_r, z_x) = array_refs![bytes, 32, 32, 32, 32, 32];

        let Y_0 = CompressedRistretto::from_slice(Y_0);
        let Y_1 = CompressedRistretto::from_slice(Y_1);
        let Y_2 = CompressedRistretto::from_slice(Y_2);

        let z_r = Scalar::from_canonical_bytes(*z_r).ok_or(ValidityProofError::Format)?;
        let z_x = Scalar::from_canonical_bytes(*z_x).ok_or(ValidityProofError::Format)?;

        Ok(ValidityProof {
            Y_0,
            Y_1,
            Y_2,
            z_r,
            z_x,
        })
    }
}

/// Aggregated ciphertext validity proof.
///
/// An aggregated ciphertext validity proof certifies the validity of two instances of a standard
/// ciphertext validity proof. An instance of a standard validity proof consist of one ciphertext
/// and two decryption handles `(commitment, handle_dest, handle_auditor)`. An instance of an
/// aggregated ciphertext validity proof is a pair `(commitment_0, handle_dest_0,
/// handle_auditor_0)` and `(commitment_1, handle_dest_1, handle_auditor_1)`. The proof certifies
/// the analogous decryptable properties for each one of these pair of commitment and decryption
/// handles.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct AggregatedValidityProof(ValidityProof);

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl AggregatedValidityProof {
    /// Aggregated ciphertext validity proof constructor.
    ///
    /// The function simples aggregates the input openings and invokes the standard ciphertext
    /// validity proof constructor.
    pub fn new<T: Into<Scalar>>(
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        (amount_lo, amount_hi): (T, T),
        (opening_lo, opening_hi): (&PedersenOpening, &PedersenOpening),
        transcript: &mut Transcript,
    ) -> Self {
        let t = transcript.challenge_scalar(b"t");

        let aggregated_message = amount_lo.into() + amount_hi.into() * t;
        let aggregated_opening = opening_lo + &(opening_hi * &t);

        AggregatedValidityProof(ValidityProof::new(
            (pubkey_dest, pubkey_auditor),
            aggregated_message,
            &aggregated_opening,
            transcript,
        ))
    }

    /// Aggregated ciphertext validity proof verifier.
    ///
    /// The function does *not* hash the public keys, commitment, or decryption handles into the
    /// transcript. For security, the caller (the main protocol) should hash these public
    /// components prior to invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    pub fn verify(
        self,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        (commitment_lo, commitment_hi): (&PedersenCommitment, &PedersenCommitment),
        (handle_lo_dest, handle_hi_dest): (&DecryptHandle, &DecryptHandle),
        (handle_lo_auditor, handle_hi_auditor): (&DecryptHandle, &DecryptHandle),
        transcript: &mut Transcript,
    ) -> Result<(), ValidityProofError> {
        let t = transcript.challenge_scalar(b"t");

        let aggregated_commitment = commitment_lo + commitment_hi * t;
        let aggregated_handle_dest = handle_lo_dest + handle_hi_dest * t;
        let aggregated_handle_auditor = handle_lo_auditor + handle_hi_auditor * t;

        let AggregatedValidityProof(validity_proof) = self;

        validity_proof.verify(
            &aggregated_commitment,
            (pubkey_dest, pubkey_auditor),
            (&aggregated_handle_dest, &aggregated_handle_auditor),
            transcript,
        )
    }

    pub fn to_bytes(&self) -> [u8; 160] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidityProofError> {
        ValidityProof::from_bytes(bytes).map(Self)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encryption::{elgamal::ElGamalKeypair, pedersen::Pedersen};

    #[test]
    fn test_validity_proof_correctness() {
        let elgamal_pubkey_dest = ElGamalKeypair::new_rand().public;
        let elgamal_pubkey_auditor = ElGamalKeypair::new_rand().public;

        let amount: u64 = 55;
        let (commitment, opening) = Pedersen::new(amount);

        let handle_dest = elgamal_pubkey_dest.decrypt_handle(&opening);
        let handle_auditor = elgamal_pubkey_auditor.decrypt_handle(&opening);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = ValidityProof::new(
            (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
            amount,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment,
                (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
                (&handle_dest, &handle_auditor),
                &mut transcript_verifier,
            )
            .is_ok());
    }

    #[test]
    fn test_validity_proof_edge_cases() {
        // if destination public key zeroed, then the proof should always reject
        let elgamal_pubkey_dest = ElGamalPubkey::from_bytes(&[0u8; 32]).unwrap();
        let elgamal_pubkey_auditor = ElGamalKeypair::new_rand().public;

        let amount: u64 = 55;
        let (commitment, opening) = Pedersen::new(amount);

        let handle_dest = elgamal_pubkey_dest.decrypt_handle(&opening);
        let handle_auditor = elgamal_pubkey_auditor.decrypt_handle(&opening);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = ValidityProof::new(
            (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
            amount,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment,
                (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
                (&handle_dest, &handle_auditor),
                &mut transcript_verifier,
            )
            .is_err());

        // if auditor public key zeroed, then the proof should always reject
        let elgamal_pubkey_dest = ElGamalKeypair::new_rand().public;
        let elgamal_pubkey_auditor = ElGamalPubkey::from_bytes(&[0u8; 32]).unwrap();

        let amount: u64 = 55;
        let (commitment, opening) = Pedersen::new(amount);

        let handle_dest = elgamal_pubkey_dest.decrypt_handle(&opening);
        let handle_auditor = elgamal_pubkey_auditor.decrypt_handle(&opening);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = ValidityProof::new(
            (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
            amount,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment,
                (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
                (&handle_dest, &handle_auditor),
                &mut transcript_verifier,
            )
            .is_err());

        // all zeroed ciphertext should still be valid
        let elgamal_pubkey_dest = ElGamalKeypair::new_rand().public;
        let elgamal_pubkey_auditor = ElGamalKeypair::new_rand().public;

        let amount: u64 = 0;
        let commitment = PedersenCommitment::from_bytes(&[0u8; 32]).unwrap();
        let opening = PedersenOpening::from_bytes(&[0u8; 32]).unwrap();

        let handle_dest = elgamal_pubkey_dest.decrypt_handle(&opening);
        let handle_auditor = elgamal_pubkey_auditor.decrypt_handle(&opening);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = ValidityProof::new(
            (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
            amount,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment,
                (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
                (&handle_dest, &handle_auditor),
                &mut transcript_verifier,
            )
            .is_ok());

        // decryption handles can be zero as long as the Pedersen commitment is valid
        let elgamal_pubkey_dest = ElGamalKeypair::new_rand().public;
        let elgamal_pubkey_auditor = ElGamalKeypair::new_rand().public;

        let amount: u64 = 55;
        let (commitment, opening) = Pedersen::new(amount);

        let handle_dest = elgamal_pubkey_dest.decrypt_handle(&opening);
        let handle_auditor = elgamal_pubkey_auditor.decrypt_handle(&opening);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = ValidityProof::new(
            (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
            amount,
            &opening,
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &commitment,
                (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
                (&handle_dest, &handle_auditor),
                &mut transcript_verifier,
            )
            .is_ok());
    }

    #[test]
    fn test_aggregated_validity_proof() {
        let elgamal_pubkey_dest = ElGamalKeypair::new_rand().public;
        let elgamal_pubkey_auditor = ElGamalKeypair::new_rand().public;

        let amount_lo: u64 = 55;
        let amount_hi: u64 = 77;

        let (commitment_lo, open_lo) = Pedersen::new(amount_lo);
        let (commitment_hi, open_hi) = Pedersen::new(amount_hi);

        let handle_lo_dest = elgamal_pubkey_dest.decrypt_handle(&open_lo);
        let handle_hi_dest = elgamal_pubkey_dest.decrypt_handle(&open_hi);

        let handle_lo_auditor = elgamal_pubkey_auditor.decrypt_handle(&open_lo);
        let handle_hi_auditor = elgamal_pubkey_auditor.decrypt_handle(&open_hi);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = AggregatedValidityProof::new(
            (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
            (amount_lo, amount_hi),
            (&open_lo, &open_hi),
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                (&elgamal_pubkey_dest, &elgamal_pubkey_auditor),
                (&commitment_lo, &commitment_hi),
                (&handle_lo_dest, &handle_hi_dest),
                (&handle_lo_auditor, &handle_hi_auditor),
                &mut transcript_verifier,
            )
            .is_ok());
    }
}
