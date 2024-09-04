//! The batched ciphertext with 3 handles validity sigma proof system.
//!
//! A batched grouped ciphertext validity proof certifies the validity of two instances of a
//! standard grouped ciphertext validity proof. An instance of a standard grouped ciphertext
//! with 3 handles validity proof consists of one ciphertext and three decryption handles:
//! `(commitment, first_handle, second_handle, third_handle)`. An instance of a batched
//! grouped ciphertext with 3 handles validity proof consist of a pair of `(commitment_0,
//! first_handle_0, second_handle_0, third_handle_0)` and `(commitment_1, first_handle_1,
//! second_handle_1, third_handle_1)`. The proof certifies the anagolous decryptable
//! properties for each one of these pairs of commitment and decryption handles.
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
            grouped_ciphertext_validity::GroupedCiphertext3HandlesValidityProof,
        },
        transcript::TranscriptProtocol,
        UNIT_LEN,
    },
    curve25519_dalek::scalar::Scalar,
    merlin::Transcript,
};

/// Byte length of a batched grouped ciphertext validity proof for 3 handles
#[allow(dead_code)]
const BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN: usize = UNIT_LEN * 6;

/// Batched grouped ciphertext validity proof with two handles.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct BatchedGroupedCiphertext3HandlesValidityProof(GroupedCiphertext3HandlesValidityProof);

#[allow(non_snake_case)]
#[allow(dead_code)]
#[cfg(not(target_os = "solana"))]
impl BatchedGroupedCiphertext3HandlesValidityProof {
    /// Creates a batched grouped ciphertext validity proof.
    ///
    /// The function simply batches the input openings and invokes the standard grouped ciphertext
    /// validity proof constructor.
    pub fn new<T: Into<Scalar>>(
        first_pubkey: &ElGamalPubkey,
        second_pubkey: &ElGamalPubkey,
        third_pubkey: &ElGamalPubkey,
        amount_lo: T,
        amount_hi: T,
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.batched_grouped_ciphertext_validity_proof_domain_separator(3);

        let t = transcript.challenge_scalar(b"t");

        let batched_message = amount_lo.into() + amount_hi.into() * t;
        let batched_opening = opening_lo + &(opening_hi * &t);

        BatchedGroupedCiphertext3HandlesValidityProof(GroupedCiphertext3HandlesValidityProof::new(
            first_pubkey,
            second_pubkey,
            third_pubkey,
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
    #[allow(clippy::too_many_arguments)]
    pub fn verify(
        self,
        first_pubkey: &ElGamalPubkey,
        second_pubkey: &ElGamalPubkey,
        third_pubkey: &ElGamalPubkey,
        commitment_lo: &PedersenCommitment,
        commitment_hi: &PedersenCommitment,
        first_handle_lo: &DecryptHandle,
        first_handle_hi: &DecryptHandle,
        second_handle_lo: &DecryptHandle,
        second_handle_hi: &DecryptHandle,
        third_handle_lo: &DecryptHandle,
        third_handle_hi: &DecryptHandle,
        transcript: &mut Transcript,
    ) -> Result<(), ValidityProofVerificationError> {
        transcript.batched_grouped_ciphertext_validity_proof_domain_separator(3);

        let t = transcript.challenge_scalar(b"t");

        let batched_commitment = commitment_lo + commitment_hi * t;
        let first_batched_handle = first_handle_lo + first_handle_hi * t;
        let second_batched_handle = second_handle_lo + second_handle_hi * t;
        let third_batched_handle = third_handle_lo + third_handle_hi * t;

        let BatchedGroupedCiphertext3HandlesValidityProof(validity_proof) = self;

        validity_proof.verify(
            &batched_commitment,
            first_pubkey,
            second_pubkey,
            third_pubkey,
            &first_batched_handle,
            &second_batched_handle,
            &third_batched_handle,
            transcript,
        )
    }

    pub fn to_bytes(&self) -> [u8; BATCHED_GROUPED_CIPHERTEXT_3_HANDLES_VALIDITY_PROOF_LEN] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidityProofVerificationError> {
        GroupedCiphertext3HandlesValidityProof::from_bytes(bytes).map(Self)
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
            sigma_proofs::pod::PodBatchedGroupedCiphertext3HandlesValidityProof,
        },
        std::str::FromStr,
    };

    #[test]
    fn test_batched_grouped_ciphertext_3_handles_validity_proof() {
        let first_keypair = ElGamalKeypair::new_rand();
        let first_pubkey = first_keypair.pubkey();

        let second_keyapir = ElGamalKeypair::new_rand();
        let second_pubkey = second_keyapir.pubkey();

        let third_keypair = ElGamalKeypair::new_rand();
        let third_pubkey = third_keypair.pubkey();

        let amount_lo: u64 = 55;
        let amount_hi: u64 = 77;

        let (commitment_lo, open_lo) = Pedersen::new(amount_lo);
        let (commitment_hi, open_hi) = Pedersen::new(amount_hi);

        let first_handle_lo = first_pubkey.decrypt_handle(&open_lo);
        let first_handle_hi = first_pubkey.decrypt_handle(&open_hi);

        let second_handle_lo = second_pubkey.decrypt_handle(&open_lo);
        let second_handle_hi = second_pubkey.decrypt_handle(&open_hi);

        let third_handle_lo = third_pubkey.decrypt_handle(&open_lo);
        let third_handle_hi = third_pubkey.decrypt_handle(&open_hi);

        let mut prover_transcript = Transcript::new(b"Test");
        let mut verifier_transcript = Transcript::new(b"Test");

        let proof = BatchedGroupedCiphertext3HandlesValidityProof::new(
            first_pubkey,
            second_pubkey,
            third_pubkey,
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
                third_pubkey,
                &commitment_lo,
                &commitment_hi,
                &first_handle_lo,
                &first_handle_hi,
                &second_handle_lo,
                &second_handle_hi,
                &third_handle_lo,
                &third_handle_hi,
                &mut verifier_transcript,
            )
            .unwrap();
    }

    #[test]
    fn test_batched_grouped_ciphertext_3_handles_validity_proof_string() {
        let first_pubkey_str = "PFQ4AD4W/Y4BEg3nI/qckFLhnjMQ12xPHyaMg9Bkg3w=";
        let pod_first_pubkey = PodElGamalPubkey::from_str(first_pubkey_str).unwrap();
        let first_pubkey: ElGamalPubkey = pod_first_pubkey.try_into().unwrap();

        let second_pubkey_str = "2CZ4h5oK7zh4/3P6s/kCQoNlpUPk1IrsrAtTWjCtfFo=";
        let pod_second_pubkey = PodElGamalPubkey::from_str(second_pubkey_str).unwrap();
        let second_pubkey: ElGamalPubkey = pod_second_pubkey.try_into().unwrap();

        let third_pubkey_str = "yonKhqkoXNvMbN/tU6fjHFhfZuNPpvMj8L55aP2bBG4=";
        let pod_third_pubkey = PodElGamalPubkey::from_str(third_pubkey_str).unwrap();
        let third_pubkey: ElGamalPubkey = pod_third_pubkey.try_into().unwrap();

        let commitment_lo_str = "atIteiveexponnuF2Z1nbovZYYtcGWjglpEA3caMShM=";
        let pod_commitment_lo = PodPedersenCommitment::from_str(commitment_lo_str).unwrap();
        let commitment_lo: PedersenCommitment = pod_commitment_lo.try_into().unwrap();

        let commitment_hi_str = "IoZlSj7spae2ogiAUiEuuwAjYA5khgBH8FhaHzkh+lc=";
        let pod_commitment_hi = PodPedersenCommitment::from_str(commitment_hi_str).unwrap();
        let commitment_hi: PedersenCommitment = pod_commitment_hi.try_into().unwrap();

        let first_handle_lo_str = "6PlKiitdapVZnh7VccQNbskXop9nmITGppLsV42UMkU=";
        let pod_first_handle_lo_str = PodDecryptHandle::from_str(first_handle_lo_str).unwrap();
        let first_handle_lo: DecryptHandle = pod_first_handle_lo_str.try_into().unwrap();

        let first_handle_hi_str = "vF+oZ3WWnrJyJ95Wl8EW+aVJiFmruiuRw6+TT3QVMBI=";
        let pod_first_handle_hi_str = PodDecryptHandle::from_str(first_handle_hi_str).unwrap();
        let first_handle_hi: DecryptHandle = pod_first_handle_hi_str.try_into().unwrap();

        let second_handle_lo_str = "rvxzo5ZyrD6YTm7X3GjplgOGJjx6PtoZ+DKbL4LsQWA=";
        let pod_second_handle_lo_str = PodDecryptHandle::from_str(second_handle_lo_str).unwrap();
        let second_handle_lo: DecryptHandle = pod_second_handle_lo_str.try_into().unwrap();

        let second_handle_hi_str = "0mdZSGiWQhOjqsExqFMD8hfgUlRRRrF/G3CJ7d0LEEk=";
        let pod_second_handle_hi_str = PodDecryptHandle::from_str(second_handle_hi_str).unwrap();
        let second_handle_hi: DecryptHandle = pod_second_handle_hi_str.try_into().unwrap();

        let third_handle_lo_str = "bpT2LuFektFhI/sacjSsqNtCsO8ac5qn0jWeMeQq4WM=";
        let pod_third_handle_lo_str = PodDecryptHandle::from_str(third_handle_lo_str).unwrap();
        let third_handle_lo: DecryptHandle = pod_third_handle_lo_str.try_into().unwrap();

        let third_handle_hi_str = "OE8z7Bbv2AHnjxebK6ASJfkJbOlYQdnN6ZPkG2u4SnA=";
        let pod_third_handle_hi_str = PodDecryptHandle::from_str(third_handle_hi_str).unwrap();
        let third_handle_hi: DecryptHandle = pod_third_handle_hi_str.try_into().unwrap();

        let proof_str = "GkjZ7QKcJq5X/OU8wb26wZ7p2D9thVK+Cb11CzRjWUoihYvGfuCbVG1vr4qtnfx65SS4jVK1H0q/948A9wy8ZPTrOZJA122G4+cpt5mKnSrKq/vbv4ZRha0oR9RGJFZ2SPT3gx2jysKDKRAQgBLOzSGfQg9Hsbz57i55SQfliUF5mByZKuzGKHSIHi81BDqbrFAj6x5bOeMAaLqsCboCA5XGDUZ2HMPUGuAd9F+OaVH+eJZnuoDjwwcBQ2eANgMB";
        let pod_proof =
            PodBatchedGroupedCiphertext3HandlesValidityProof::from_str(proof_str).unwrap();
        let proof: BatchedGroupedCiphertext3HandlesValidityProof = pod_proof.try_into().unwrap();

        let mut verifier_transcript = Transcript::new(b"Test");

        proof
            .verify(
                &first_pubkey,
                &second_pubkey,
                &third_pubkey,
                &commitment_lo,
                &commitment_hi,
                &first_handle_lo,
                &first_handle_hi,
                &second_handle_lo,
                &second_handle_hi,
                &third_handle_lo,
                &third_handle_hi,
                &mut verifier_transcript,
            )
            .unwrap();
    }
}
