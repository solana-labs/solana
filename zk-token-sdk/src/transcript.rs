use {
    crate::errors::TranscriptError,
    curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar, traits::IsIdentity},
    merlin::Transcript,
};

pub trait TranscriptProtocol {
    /// Append a domain separator for an `n`-bit rangeproof for ElGamalKeypair
    /// ciphertext using a decryption key
    fn rangeproof_from_key_domain_sep(&mut self, n: u64);

    /// Append a domain separator for an `n`-bit rangeproof for ElGamalKeypair
    /// ciphertext using an opening
    fn rangeproof_from_opening_domain_sep(&mut self, n: u64);

    /// Append a domain separator for a length-`n` inner product proof.
    fn innerproduct_domain_sep(&mut self, n: u64);

    /// Append a domain separator for close account proof.
    fn close_account_proof_domain_sep(&mut self);

    /// Append a domain separator for update account public key proof.
    fn update_account_public_key_proof_domain_sep(&mut self);

    /// Append a domain separator for withdraw proof.
    fn withdraw_proof_domain_sep(&mut self);

    /// Append a domain separator for transfer proof.
    fn transfer_proof_domain_sep(&mut self);

    /// Append a `scalar` with the given `label`.
    fn append_scalar(&mut self, label: &'static [u8], scalar: &Scalar);

    /// Append a `point` with the given `label`.
    fn append_point(&mut self, label: &'static [u8], point: &CompressedRistretto);

    /// Check that a point is not the identity, then append it to the
    /// transcript.  Otherwise, return an error.
    fn validate_and_append_point(
        &mut self,
        label: &'static [u8],
        point: &CompressedRistretto,
    ) -> Result<(), TranscriptError>;

    /// Compute a `label`ed challenge variable.
    fn challenge_scalar(&mut self, label: &'static [u8]) -> Scalar;
}

impl TranscriptProtocol for Transcript {
    fn rangeproof_from_key_domain_sep(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"rangeproof from opening v1");
        self.append_u64(b"n", n);
    }

    fn rangeproof_from_opening_domain_sep(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"rangeproof from opening v1");
        self.append_u64(b"n", n);
    }

    fn innerproduct_domain_sep(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"ipp v1");
        self.append_u64(b"n", n);
    }

    fn close_account_proof_domain_sep(&mut self) {
        self.append_message(b"dom-sep", b"CloseAccountProof");
    }

    fn update_account_public_key_proof_domain_sep(&mut self) {
        self.append_message(b"dom-sep", b"UpdateAccountPublicKeyProof");
    }

    fn withdraw_proof_domain_sep(&mut self) {
        self.append_message(b"dom-sep", b"WithdrawProof");
    }

    fn transfer_proof_domain_sep(&mut self) {
        self.append_message(b"dom-sep", b"TransferProof");
    }

    fn append_scalar(&mut self, label: &'static [u8], scalar: &Scalar) {
        self.append_message(label, scalar.as_bytes());
    }

    fn append_point(&mut self, label: &'static [u8], point: &CompressedRistretto) {
        self.append_message(label, point.as_bytes());
    }

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

    fn challenge_scalar(&mut self, label: &'static [u8]) -> Scalar {
        let mut buf = [0u8; 64];
        self.challenge_bytes(label, &mut buf);

        Scalar::from_bytes_mod_order_wide(&buf)
    }
}
