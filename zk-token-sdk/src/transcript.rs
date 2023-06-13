use {
    crate::{errors::TranscriptError, zk_token_elgamal::pod},
    curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar, traits::IsIdentity},
    merlin::Transcript,
};

pub trait TranscriptProtocol {
    /// Append a domain separator for an `n`-bit rangeproof for ElGamalKeypair
    /// ciphertext using a decryption key
    fn rangeproof_from_key_domain_separator(&mut self, n: u64);

    /// Append a domain separator for an `n`-bit rangeproof for ElGamalKeypair
    /// ciphertext using an opening
    fn rangeproof_from_opening_domain_separator(&mut self, n: u64);

    /// Append a domain separator for a length-`n` inner product proof.
    fn innerproduct_domain_separator(&mut self, n: u64);

    /// Append a domain separator for close account proof.
    fn close_account_proof_domain_separator(&mut self);

    /// Append a domain separator for withdraw proof.
    fn withdraw_proof_domain_separator(&mut self);

    /// Append a domain separator for transfer proof.
    fn transfer_proof_domain_separator(&mut self);

    /// Append a `scalar` with the given `label`.
    fn append_scalar(&mut self, label: &'static [u8], scalar: &Scalar);

    /// Append a `point` with the given `label`.
    fn append_point(&mut self, label: &'static [u8], point: &CompressedRistretto);

    /// Append an ElGamal pubkey with the given `label`.
    fn append_pubkey(&mut self, label: &'static [u8], point: &pod::ElGamalPubkey);

    /// Append an ElGamal ciphertext with the given `label`.
    fn append_ciphertext(&mut self, label: &'static [u8], point: &pod::ElGamalCiphertext);

    /// Append a grouped ElGamal ciphertext with the given `label`.
    fn append_grouped_ciphertext_2_handles(
        &mut self,
        label: &'static [u8],
        point: &pod::GroupedElGamalCiphertext2Handles,
    );

    /// Append a Pedersen commitment with the given `label`.
    fn append_commitment(&mut self, label: &'static [u8], point: &pod::PedersenCommitment);

    /// Append an ElGamal decryption handle with the given `label`.
    fn append_handle(&mut self, label: &'static [u8], point: &pod::DecryptHandle);

    /// Append a domain separator for equality proof.
    fn equality_proof_domain_separator(&mut self);

    /// Append a domain separator for zero-balance proof.
    fn zero_balance_proof_domain_separator(&mut self);

    /// Append a domain separator for grouped ciphertext validity proof.
    fn grouped_ciphertext_validity_proof_domain_separator(&mut self);

    /// Append a domain separator for batched grouped ciphertext validity proof.
    fn batched_grouped_ciphertext_validity_proof_domain_separator(&mut self);

    /// Append a domain separator for fee sigma proof.
    fn fee_sigma_proof_domain_separator(&mut self);

    /// Append a domain separator for public-key proof.
    fn pubkey_proof_domain_separator(&mut self);

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
    fn rangeproof_from_key_domain_separator(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"rangeproof from opening v1");
        self.append_u64(b"n", n);
    }

    fn rangeproof_from_opening_domain_separator(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"rangeproof from opening v1");
        self.append_u64(b"n", n);
    }

    fn innerproduct_domain_separator(&mut self, n: u64) {
        self.append_message(b"dom-sep", b"ipp v1");
        self.append_u64(b"n", n);
    }

    fn close_account_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"CloseAccountProof");
    }

    fn withdraw_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"WithdrawProof");
    }

    fn transfer_proof_domain_separator(&mut self) {
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

    fn append_pubkey(&mut self, label: &'static [u8], pubkey: &pod::ElGamalPubkey) {
        self.append_message(label, &pubkey.0);
    }

    fn append_ciphertext(&mut self, label: &'static [u8], ciphertext: &pod::ElGamalCiphertext) {
        self.append_message(label, &ciphertext.0);
    }

    fn append_grouped_ciphertext_2_handles(
        &mut self,
        label: &'static [u8],
        grouped_ciphertext: &pod::GroupedElGamalCiphertext2Handles,
    ) {
        self.append_message(label, &grouped_ciphertext.0);
    }

    fn append_commitment(&mut self, label: &'static [u8], commitment: &pod::PedersenCommitment) {
        self.append_message(label, &commitment.0);
    }

    fn append_handle(&mut self, label: &'static [u8], handle: &pod::DecryptHandle) {
        self.append_message(label, &handle.0);
    }

    fn equality_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"equality-proof")
    }

    fn zero_balance_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"zero-balance-proof")
    }

    fn grouped_ciphertext_validity_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"validity-proof")
    }

    fn batched_grouped_ciphertext_validity_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"batched-validity-proof")
    }

    fn fee_sigma_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"fee-sigma-proof")
    }

    fn pubkey_proof_domain_separator(&mut self) {
        self.append_message(b"dom-sep", b"pubkey-proof")
    }
}
