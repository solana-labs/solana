use merlin::Transcript;
#[cfg(not(target_os = "solana"))]
use {
    crate::errors::TranscriptError,
    curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar, traits::IsIdentity},
};

pub trait TranscriptProtocol {
    /// Append a `scalar` with the given `label`.
    #[cfg(not(target_os = "solana"))]
    fn append_scalar(&mut self, label: &'static [u8], scalar: &Scalar);

    /// Append a `point` with the given `label`.
    #[cfg(not(target_os = "solana"))]
    fn append_point(&mut self, label: &'static [u8], point: &CompressedRistretto);

    /// Check that a point is not the identity, then append it to the
    /// transcript.  Otherwise, return an error.
    #[cfg(not(target_os = "solana"))]
    fn validate_and_append_point(
        &mut self,
        label: &'static [u8],
        point: &CompressedRistretto,
    ) -> Result<(), TranscriptError>;

    /// Append a domain separator for an `n`-bit range proof
    fn range_proof_domain_separator(&mut self, n: u64);

    /// Append a domain separator for a length-`n` inner product proof.
    fn inner_product_proof_domain_separator(&mut self, n: u64);

    /// Append a domain separator for ciphertext-ciphertext equality proof.
    fn ciphertext_ciphertext_equality_proof_domain_separator(&mut self);

    /// Append a domain separator for ciphertext-commitment equality proof.
    fn ciphertext_commitment_equality_proof_domain_separator(&mut self);

    /// Append a domain separator for zero-ciphertext proof.
    fn zero_ciphertext_proof_domain_separator(&mut self);

    /// Append a domain separator for grouped ciphertext validity proof.
    fn grouped_ciphertext_validity_proof_domain_separator(&mut self, handles: u64);

    /// Append a domain separator for batched grouped ciphertext validity proof.
    fn batched_grouped_ciphertext_validity_proof_domain_separator(&mut self, handles: u64);

    /// Append a domain separator for percentage with cap proof.
    fn percentage_with_cap_proof_domain_separator(&mut self);

    /// Append a domain separator for public-key proof.
    fn pubkey_proof_domain_separator(&mut self);

    /// Compute a `label`ed challenge variable.
    #[cfg(not(target_os = "solana"))]
    fn challenge_scalar(&mut self, label: &'static [u8]) -> Scalar;
}

impl TranscriptProtocol for Transcript {
    #[cfg(not(target_os = "solana"))]
    fn append_scalar(&mut self, label: &'static [u8], scalar: &Scalar) {
        self.append_message(label, scalar.as_bytes());
    }

    #[cfg(not(target_os = "solana"))]
    fn append_point(&mut self, label: &'static [u8], point: &CompressedRistretto) {
        self.append_message(label, point.as_bytes());
    }

    #[cfg(not(target_os = "solana"))]
    fn validate_and_append_point(
        &mut self,
        label: &'static [u8],
        point: &CompressedRistretto,
    ) -> Result<(), TranscriptError> {
        if point.is_identity() {
            Err(TranscriptError::ValidationError)
        } else {
            self.append_message(label, point.as_bytes());
            Ok(())
        }
    }

    fn range_proof_domain_separator(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"range-proof");
        self.append_u64(b"n", n);
    }

    fn inner_product_proof_domain_separator(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"inner-product");
        self.append_u64(b"n", n);
    }

    fn ciphertext_ciphertext_equality_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"ciphertext-ciphertext-equality-proof")
    }

    fn ciphertext_commitment_equality_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"ciphertext-commitment-equality-proof")
    }

    fn zero_ciphertext_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"zero-ciphertext-proof")
    }

    fn grouped_ciphertext_validity_proof_domain_separator(&mut self, handles: u64) {
        self.append_message(b"dom-sep", b"validity-proof");
        self.append_u64(b"handles", handles);
    }

    fn batched_grouped_ciphertext_validity_proof_domain_separator(&mut self, handles: u64) {
        self.append_message(b"dom-sep", b"batched-validity-proof");
        self.append_u64(b"handles", handles);
    }

    fn percentage_with_cap_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"percentage-with-cap-proof")
    }

    fn pubkey_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"pubkey-proof")
    }

    #[cfg(not(target_os = "solana"))]
    fn challenge_scalar(&mut self, label: &'static [u8]) -> Scalar {
        let mut buf = [0u8; 64];
        self.challenge_bytes(label, &mut buf);

        Scalar::from_bytes_mod_order_wide(&buf)
    }
}
