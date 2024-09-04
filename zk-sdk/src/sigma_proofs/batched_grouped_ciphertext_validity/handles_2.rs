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
            errors::ValidityProofVerificationError,
            grouped_ciphertext_validity::GroupedCiphertext2HandlesValidityProof,
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
/// ciphertext and two decryption handles: `(commitment, first_handle, second_handle)`. An
/// instance of a batched ciphertext validity proof is a pair `(commitment_0,
/// first_handle_0, second_handle_0)` and `(commitment_1, first_handle_1,
/// second_handle_1)`. The proof certifies the analogous decryptable properties for each one of
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
    ///
    /// This function is randomized. It uses `OsRng` internally to generate random scalars.
    pub fn new<T: Into<Scalar>>(
        first_pubkey: &ElGamalPubkey,
        second_pubkey: &ElGamalPubkey,
        amount_lo: T,
        amount_hi: T,
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.batched_grouped_ciphertext_validity_proof_domain_separator(2);

        let t = transcript.challenge_scalar(b"t");

        let batched_message = amount_lo.into() + amount_hi.into() * t;
        let batched_opening = opening_lo + &(opening_hi * &t);

        BatchedGroupedCiphertext2HandlesValidityProof(GroupedCiphertext2HandlesValidityProof::new(
            first_pubkey,
            second_pubkey,
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
    #[allow(clippy::too_many_arguments)]
    pub fn verify(
        self,
        first_pubkey: &ElGamalPubkey,
        second_pubkey: &ElGamalPubkey,
        commitment_lo: &PedersenCommitment,
        commitment_hi: &PedersenCommitment,
        first_handle_lo: &DecryptHandle,
        first_handle_hi: &DecryptHandle,
        second_handle_lo: &DecryptHandle,
        second_handle_hi: &DecryptHandle,
        transcript: &mut Transcript,
    ) -> Result<(), ValidityProofVerificationError> {
        transcript.batched_grouped_ciphertext_validity_proof_domain_separator(2);

        let t = transcript.challenge_scalar(b"t");

        let batched_commitment = commitment_lo + commitment_hi * t;
        let first_batched_handle = first_handle_lo + first_handle_hi * t;
        let second_batched_handle = second_handle_lo + second_handle_hi * t;

        let BatchedGroupedCiphertext2HandlesValidityProof(validity_proof) = self;

        validity_proof.verify(
            &batched_commitment,
            first_pubkey,
            second_pubkey,
            &first_batched_handle,
            &second_batched_handle,
            transcript,
        )
    }

    pub fn to_bytes(&self) -> [u8; 160] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidityProofVerificationError> {
        GroupedCiphertext2HandlesValidityProof::from_bytes(bytes).map(Self)
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{
            encryption::{
                elgamal::ElGamalKeypair,
                pedersen::Pedersen,
                pod::{
                    elgamal::{PodDecryptHandle, PodElGamalPubkey},
                    pedersen::PodPedersenCommitment,
                },
            },
            sigma_proofs::pod::PodBatchedGroupedCiphertext2HandlesValidityProof,
        },
        std::str::FromStr,
    };

    #[test]
    fn test_batched_grouped_ciphertext_2_handles_validity_proof() {
        let first_keypair = ElGamalKeypair::new_rand();
        let first_pubkey = first_keypair.pubkey();

        let second_keypair = ElGamalKeypair::new_rand();
        let second_pubkey = second_keypair.pubkey();

        let amount_lo: u64 = 55;
        let amount_hi: u64 = 77;

        let (commitment_lo, open_lo) = Pedersen::new(amount_lo);
        let (commitment_hi, open_hi) = Pedersen::new(amount_hi);

        let first_handle_lo = first_pubkey.decrypt_handle(&open_lo);
        let first_handle_hi = first_pubkey.decrypt_handle(&open_hi);

        let second_handle_lo = second_pubkey.decrypt_handle(&open_lo);
        let second_handle_hi = second_pubkey.decrypt_handle(&open_hi);

        let mut prover_transcript = Transcript::new(b"Test");
        let mut verifier_transcript = Transcript::new(b"Test");

        let proof = BatchedGroupedCiphertext2HandlesValidityProof::new(
            first_pubkey,
            second_pubkey,
            amount_lo,
            amount_hi,
            &open_lo,
            &open_hi,
            &mut prover_transcript,
        );

        proof
            .verify(
                first_pubkey,
                second_pubkey,
                &commitment_lo,
                &commitment_hi,
                &first_handle_lo,
                &first_handle_hi,
                &second_handle_lo,
                &second_handle_hi,
                &mut verifier_transcript,
            )
            .unwrap();
    }

    #[test]
    fn test_batched_grouped_ciphertext_2_handles_validity_proof_string() {
        let first_pubkey_str = "3FQGicS6AgVkRnX5Sau8ybxJDvlehmbdvBUdo+o+oE4=";
        let pod_first_pubkey = PodElGamalPubkey::from_str(first_pubkey_str).unwrap();
        let first_pubkey: ElGamalPubkey = pod_first_pubkey.try_into().unwrap();

        let second_pubkey_str = "IieU/fJCRksbDNvIJZvg/N/safpnIWAGT/xpUAG7YUg=";
        let pod_second_pubkey = PodElGamalPubkey::from_str(second_pubkey_str).unwrap();
        let second_pubkey: ElGamalPubkey = pod_second_pubkey.try_into().unwrap();

        let commitment_lo_str = "Lq0z7bx3ccyxIB0rRHoWzcba8W1azvAhMfnJogxcz2I=";
        let pod_commitment_lo = PodPedersenCommitment::from_str(commitment_lo_str).unwrap();
        let commitment_lo: PedersenCommitment = pod_commitment_lo.try_into().unwrap();

        let commitment_hi_str = "dLPLdQrcl5ZWb0EaJcmebAlJA6RrzKpMSYPDVMJdOm0=";
        let pod_commitment_hi = PodPedersenCommitment::from_str(commitment_hi_str).unwrap();
        let commitment_hi: PedersenCommitment = pod_commitment_hi.try_into().unwrap();

        let first_handle_lo_str = "GizvHRUmu6CMjhH7qWg5Rqu43V69Nyjq4QsN/yXBHT8=";
        let pod_first_handle_lo_str = PodDecryptHandle::from_str(first_handle_lo_str).unwrap();
        let first_handle_lo: DecryptHandle = pod_first_handle_lo_str.try_into().unwrap();

        let first_handle_hi_str = "qMuR929bbkKiVJfRvYxnb90rbh2btjNDjaXpeLCvQWk=";
        let pod_first_handle_hi_str = PodDecryptHandle::from_str(first_handle_hi_str).unwrap();
        let first_handle_hi: DecryptHandle = pod_first_handle_hi_str.try_into().unwrap();

        let second_handle_lo_str = "MmDbMo2l/jAcXUIm09AQZsBXa93lI2BapAiGZ6f9zRs=";
        let pod_second_handle_lo_str = PodDecryptHandle::from_str(second_handle_lo_str).unwrap();
        let second_handle_lo: DecryptHandle = pod_second_handle_lo_str.try_into().unwrap();

        let second_handle_hi_str = "gKhb0o3d22XcUcQl5hENF4l1SJwg1vpgiw2RDYqXOxY=";
        let pod_second_handle_hi_str = PodDecryptHandle::from_str(second_handle_hi_str).unwrap();
        let second_handle_hi: DecryptHandle = pod_second_handle_hi_str.try_into().unwrap();

        let proof_str = "2n2mADpkNrop+eHJj1sAryXWcTtC/7QKcxMp7FdHeh8wjGKLAa9kC89QLGrphv7pZdb2J25kKXqhWUzRBsJWU0izi5vxau9XX6cyd72F3Q9hMXBfjk3htOHI0VnGAalZ/3dZ6C7erjGQDoeTVGOd1vewQ+NObAbfZwcry3+VhQNpkhL17E1dUgZZ+mb5K0tXAjWCmVh1OfN9h3sGltTUCg==";
        let pod_proof =
            PodBatchedGroupedCiphertext2HandlesValidityProof::from_str(proof_str).unwrap();
        let proof: BatchedGroupedCiphertext2HandlesValidityProof = pod_proof.try_into().unwrap();

        let mut verifier_transcript = Transcript::new(b"Test");

        proof
            .verify(
                &first_pubkey,
                &second_pubkey,
                &commitment_lo,
                &commitment_hi,
                &first_handle_lo,
                &first_handle_hi,
                &second_handle_lo,
                &second_handle_hi,
                &mut verifier_transcript,
            )
            .unwrap();
    }
}
