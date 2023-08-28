//! The ciphertext validity sigma proof system.
//!
//! The ciphertext validity proof is defined with respect to a Pedersen commitment and two
//! decryption handles. The proof certifies that a given Pedersen commitment can be decrypted using
//! ElGamal private keys that are associated with each of the two decryption handles. To generate
//! the proof, a prover must provide the Pedersen opening associated with the commitment.
//!
//! The protocol guarantees computational soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.

#[cfg(not(target_os = "solana"))]
use crate::encryption::{
    elgamal::{DecryptHandle, ElGamalPubkey},
    pedersen::{PedersenCommitment, PedersenOpening},
};
use {
    crate::{
        sigma_proofs::{
            errors::ValidityProofError,
            grouped_ciphertext_validity_proof::GroupedCiphertext2HandlesValidityProof,
        },
        transcript::TranscriptProtocol,
    },
    curve25519_dalek::scalar::Scalar,
    merlin::Transcript,
};

/// Batched grouped ciphertext validity proof with two handles.
///
/// A batched grouped ciphertext validity proof certifies the validity of two instances of a
/// standard ciphertext validity proof. An instance of a standard validity proof consists of one
/// ciphertext and two decryption handles: `(commitment, destination_handle, auditor_handle)`. An
/// instance of a batched ciphertext validity proof is a pair `(commitment_0,
/// destination_handle_0, auditor_handle_0)` and `(commitment_1, destination_handle_1,
/// auditor_handle_1)`. The proof certifies the analogous decryptable properties for each one of
/// these pairs of commitment and decryption handles.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct BatchedGroupedCiphertext2HandlesValidityProof(GroupedCiphertext2HandlesValidityProof);

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext2HandlesValidityProof {
    /// Creates a batched grouped ciphertext validity proof.
    ///
    /// The function simply batches the input openings and invokes the standard grouped ciphertext
    /// validity proof constructor.
    pub fn new<T: Into<Scalar>>(
        (destination_pubkey, auditor_pubkey): (&ElGamalPubkey, &ElGamalPubkey),
        (amount_lo, amount_hi): (T, T),
        (opening_lo, opening_hi): (&PedersenOpening, &PedersenOpening),
        transcript: &mut Transcript,
    ) -> Self {
        transcript.batched_grouped_ciphertext_validity_proof_domain_separator();

        let t = transcript.challenge_scalar(b"t");

        let batched_message = amount_lo.into() + amount_hi.into() * t;
        let batched_opening = opening_lo + &(opening_hi * &t);

        BatchedGroupedCiphertext2HandlesValidityProof(GroupedCiphertext2HandlesValidityProof::new(
            (destination_pubkey, auditor_pubkey),
            batched_message,
            &batched_opening,
            transcript,
        ))
    }

    /// Verifies a batched grouped ciphertext validity proof.
    ///
    /// The function does *not* hash the public keys, commitment, or decryption handles into the
    /// transcript. For security, the caller (the main protocol) should hash these public
    /// components prior to invoking this constructor.
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    pub fn verify(
        self,
        (destination_pubkey, auditor_pubkey): (&ElGamalPubkey, &ElGamalPubkey),
        (commitment_lo, commitment_hi): (&PedersenCommitment, &PedersenCommitment),
        (destination_handle_lo, destination_handle_hi): (&DecryptHandle, &DecryptHandle),
        (auditor_handle_lo, auditor_handle_hi): (&DecryptHandle, &DecryptHandle),
        transcript: &mut Transcript,
    ) -> Result<(), ValidityProofError> {
        transcript.batched_grouped_ciphertext_validity_proof_domain_separator();

        let t = transcript.challenge_scalar(b"t");

        let batched_commitment = commitment_lo + commitment_hi * t;
        let destination_batched_handle = destination_handle_lo + destination_handle_hi * t;
        let auditor_batched_handle = auditor_handle_lo + auditor_handle_hi * t;

        let BatchedGroupedCiphertext2HandlesValidityProof(validity_proof) = self;

        validity_proof.verify(
            &batched_commitment,
            (destination_pubkey, auditor_pubkey),
            (&destination_batched_handle, &auditor_batched_handle),
            transcript,
        )
    }

    pub fn to_bytes(&self) -> [u8; 160] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidityProofError> {
        GroupedCiphertext2HandlesValidityProof::from_bytes(bytes).map(Self)
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{elgamal::ElGamalKeypair, pedersen::Pedersen},
    };

    #[test]
    fn test_batched_grouped_ciphertext_validity_proof() {
        let destination_keypair = ElGamalKeypair::new_rand();
        let destination_pubkey = destination_keypair.pubkey();

        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pubkey = auditor_keypair.pubkey();

        let amount_lo: u64 = 55;
        let amount_hi: u64 = 77;

        let (commitment_lo, open_lo) = Pedersen::new(amount_lo);
        let (commitment_hi, open_hi) = Pedersen::new(amount_hi);

        let destination_handle_lo = destination_pubkey.decrypt_handle(&open_lo);
        let destination_handle_hi = destination_pubkey.decrypt_handle(&open_hi);

        let auditor_handle_lo = auditor_pubkey.decrypt_handle(&open_lo);
        let auditor_handle_hi = auditor_pubkey.decrypt_handle(&open_hi);

        let mut prover_transcript = Transcript::new(b"Test");
        let mut verifier_transcript = Transcript::new(b"Test");

        let proof = BatchedGroupedCiphertext2HandlesValidityProof::new(
            (destination_pubkey, auditor_pubkey),
            (amount_lo, amount_hi),
            (&open_lo, &open_hi),
            &mut prover_transcript,
        );

        assert!(proof
            .verify(
                (destination_pubkey, auditor_pubkey),
                (&commitment_lo, &commitment_hi),
                (&destination_handle_lo, &destination_handle_hi),
                (&auditor_handle_lo, &auditor_handle_hi),
                &mut verifier_transcript,
            )
            .is_ok());
    }
}
