use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            discrete_log::*,
            elgamal::{
                DecryptHandle, ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey, ElGamalSecretKey,
            },
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        instruction::{combine_u32_ciphertexts, split_u64_into_u32, Role, Verifiable, TWO_32},
        range_proof::RangeProof,
        sigma_proofs::{equality_proof::EqualityProof, validity_proof::AggregatedValidityProof},
        transcript::TranscriptProtocol,
    },
    arrayref::{array_ref, array_refs},
    merlin::Transcript,
    std::convert::TryInto,
};

#[derive(Clone)]
#[repr(C)]
#[cfg(not(target_arch = "bpf"))]
pub struct TransferAmountEncryption {
    pub commitment: PedersenCommitment,
    pub source: DecryptHandle,
    pub dest: DecryptHandle,
    pub auditor: DecryptHandle,
}

#[cfg(not(target_arch = "bpf"))]
impl TransferAmountEncryption {
    pub fn new(
        amount: u32,
        pubkey_source: &ElGamalPubkey,
        pubkey_dest: &ElGamalPubkey,
        pubkey_auditor: &ElGamalPubkey,
    ) -> (Self, PedersenOpening) {
        let (commitment, opening) = Pedersen::new(amount);
        let transfer_amount_encryption = Self {
            commitment,
            source: pubkey_source.decrypt_handle(&opening),
            dest: pubkey_dest.decrypt_handle(&opening),
            auditor: pubkey_auditor.decrypt_handle(&opening),
        };

        (transfer_amount_encryption, opening)
    }

    pub fn to_bytes(&self) -> [u8; 128] {
        let mut bytes = [0u8; 128];
        bytes[..32].copy_from_slice(&self.commitment.to_bytes());
        bytes[32..64].copy_from_slice(&self.source.to_bytes());
        bytes[64..96].copy_from_slice(&self.dest.to_bytes());
        bytes[96..128].copy_from_slice(&self.auditor.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProofError> {
        let bytes = array_ref![bytes, 0, 128];
        let (commitment, source, dest, auditor) = array_refs![bytes, 32, 32, 32, 32];

        let commitment =
            PedersenCommitment::from_bytes(commitment).ok_or(ProofError::Verification)?;
        let source = DecryptHandle::from_bytes(source).ok_or(ProofError::Verification)?;
        let dest = DecryptHandle::from_bytes(dest).ok_or(ProofError::Verification)?;
        let auditor = DecryptHandle::from_bytes(auditor).ok_or(ProofError::Verification)?;

        Ok(Self {
            commitment,
            source,
            dest,
            auditor,
        })
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferData {
    /// Group encryption of the low 32 bits of the transfer amount
    pub ciphertext_lo: pod::TransferAmountEncryption,

    /// Group encryption of the high 32 bits of the transfer amount
    pub ciphertext_hi: pod::TransferAmountEncryption,

    /// The public encryption keys associated with the transfer: source, dest, and auditor
    pub transfer_pubkeys: pod::TransferPubkeys,

    /// The final spendable ciphertext after the transfer
    pub ciphertext_new_source: pod::ElGamalCiphertext,

    /// Zero-knowledge proofs for Transfer
    pub proof: TransferProof,
}

#[cfg(not(target_arch = "bpf"))]
impl TransferData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transfer_amount: u64,
        (spendable_balance, ciphertext_old_source): (u64, &ElGamalCiphertext),
        keypair_source: &ElGamalKeypair,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
    ) -> Result<Self, ProofError> {
        // split and encrypt transfer amount
        let (amount_lo, amount_hi) = split_u64_into_u32(transfer_amount);

        let (ciphertext_lo, opening_lo) = TransferAmountEncryption::new(
            amount_lo,
            &keypair_source.public,
            pubkey_dest,
            pubkey_auditor,
        );
        let (ciphertext_hi, opening_hi) = TransferAmountEncryption::new(
            amount_hi,
            &keypair_source.public,
            pubkey_dest,
            pubkey_auditor,
        );

        // subtract transfer amount from the spendable ciphertext
        let new_spendable_balance = spendable_balance
            .checked_sub(transfer_amount)
            .ok_or(ProofError::Generation)?;

        let transfer_amount_lo_source = ElGamalCiphertext {
            commitment: ciphertext_lo.commitment,
            handle: ciphertext_lo.source,
        };

        let transfer_amount_hi_source = ElGamalCiphertext {
            commitment: ciphertext_hi.commitment,
            handle: ciphertext_hi.source,
        };

        let ciphertext_new_source = ciphertext_old_source
            - combine_u32_ciphertexts(&transfer_amount_lo_source, &transfer_amount_hi_source);

        // generate transcript and append all public inputs
        let pod_transfer_pubkeys =
            pod::TransferPubkeys::new(&keypair_source.public, pubkey_dest, pubkey_auditor);
        let pod_ciphertext_lo: pod::TransferAmountEncryption = ciphertext_lo.into();
        let pod_ciphertext_hi: pod::TransferAmountEncryption = ciphertext_hi.into();
        let pod_ciphertext_new_source: pod::ElGamalCiphertext = ciphertext_new_source.into();

        let mut transcript = TransferProof::transcript_new(
            &pod_transfer_pubkeys,
            &pod_ciphertext_lo,
            &pod_ciphertext_hi,
            &pod_ciphertext_new_source,
        );

        let proof = TransferProof::new(
            (amount_lo, amount_hi),
            keypair_source,
            (pubkey_dest, pubkey_auditor),
            &opening_lo,
            &opening_hi,
            (new_spendable_balance, &ciphertext_new_source),
            &mut transcript,
        );

        Ok(Self {
            ciphertext_lo: pod_ciphertext_lo,
            ciphertext_hi: pod_ciphertext_hi,
            transfer_pubkeys: pod_transfer_pubkeys,
            ciphertext_new_source: pod_ciphertext_new_source,
            proof,
        })
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_lo(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_lo: TransferAmountEncryption = self.ciphertext_lo.try_into()?;

        let handle_lo = match role {
            Role::Source => ciphertext_lo.source,
            Role::Dest => ciphertext_lo.dest,
            Role::Auditor => ciphertext_lo.auditor,
        };

        Ok(ElGamalCiphertext {
            commitment: ciphertext_lo.commitment,
            handle: handle_lo,
        })
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_hi(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_hi: TransferAmountEncryption = self.ciphertext_hi.try_into()?;

        let handle_hi = match role {
            Role::Source => ciphertext_hi.source,
            Role::Dest => ciphertext_hi.dest,
            Role::Auditor => ciphertext_hi.auditor,
        };

        Ok(ElGamalCiphertext {
            commitment: ciphertext_hi.commitment,
            handle: handle_hi,
        })
    }

    /// Decrypts transfer amount from transfer data
    ///
    /// TODO: This function should run in constant time. Use `subtle::Choice` for the if statement
    /// and make sure that the function does not terminate prematurely due to errors
    ///
    /// TODO: Define specific error type for decryption error
    pub fn decrypt_amount(&self, role: Role, sk: &ElGamalSecretKey) -> Result<u64, ProofError> {
        let ciphertext_lo = self.ciphertext_lo(role)?;
        let ciphertext_hi = self.ciphertext_hi(role)?;

        let amount_lo = ciphertext_lo.decrypt_u32_online(sk, &DECODE_U32_PRECOMPUTATION_FOR_G);
        let amount_hi = ciphertext_hi.decrypt_u32_online(sk, &DECODE_U32_PRECOMPUTATION_FOR_G);

        if let (Some(amount_lo), Some(amount_hi)) = (amount_lo, amount_hi) {
            Ok((amount_lo as u64) + (TWO_32 * amount_hi as u64))
        } else {
            Err(ProofError::Verification)
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for TransferData {
    fn verify(&self) -> Result<(), ProofError> {
        // generate transcript and append all public inputs
        let mut transcript = TransferProof::transcript_new(
            &self.transfer_pubkeys,
            &self.ciphertext_lo,
            &self.ciphertext_hi,
            &self.ciphertext_new_source,
        );

        let ciphertext_lo = self.ciphertext_lo.try_into()?;
        let ciphertext_hi = self.ciphertext_hi.try_into()?;
        let transfer_pubkeys = self.transfer_pubkeys.try_into()?;
        let new_spendable_ciphertext = self.ciphertext_new_source.try_into()?;

        self.proof.verify(
            &ciphertext_lo,
            &ciphertext_hi,
            &transfer_pubkeys,
            &new_spendable_ciphertext,
            &mut transcript,
        )
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferProof {
    /// New Pedersen commitment for the remaining balance in source
    pub commitment_new_source: pod::PedersenCommitment,

    /// Associated equality proof
    pub equality_proof: pod::EqualityProof,

    /// Associated ciphertext validity proof
    pub validity_proof: pod::AggregatedValidityProof,

    // Associated range proof
    pub range_proof: pod::RangeProof128,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl TransferProof {
    fn transcript_new(
        transfer_pubkeys: &pod::TransferPubkeys,
        ciphertext_lo: &pod::TransferAmountEncryption,
        ciphertext_hi: &pod::TransferAmountEncryption,
        ciphertext_new_source: &pod::ElGamalCiphertext,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"transfer-proof");

        transcript.append_message(b"transfer-pubkeys", &transfer_pubkeys.0);
        transcript.append_message(b"ciphertext-lo", &ciphertext_lo.0);
        transcript.append_message(b"ciphertext-hi", &ciphertext_hi.0);
        transcript.append_message(b"ciphertext-new-source", &ciphertext_new_source.0);

        transcript
    }

    pub fn new(
        (transfer_amount_lo, transfer_amount_hi): (u32, u32),
        keypair_source: &ElGamalKeypair,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
        (source_new_balance, ciphertext_new_source): (u64, &ElGamalCiphertext),
        transcript: &mut Transcript,
    ) -> Self {
        // generate a Pedersen commitment for the remaining balance in source
        let (commitment_new_source, opening_source) = Pedersen::new(source_new_balance);

        let pod_commitment_new_source: pod::PedersenCommitment = commitment_new_source.into();
        transcript.append_commitment(b"commitment-new-source", &pod_commitment_new_source);

        // generate equality_proof
        let equality_proof = EqualityProof::new(
            keypair_source,
            ciphertext_new_source,
            source_new_balance,
            &opening_source,
            transcript,
        );

        // generate ciphertext validity proof
        let validity_proof = AggregatedValidityProof::new(
            (pubkey_dest, pubkey_auditor),
            (transfer_amount_lo, transfer_amount_hi),
            (opening_lo, opening_hi),
            transcript,
        );

        // generate the range proof
        let range_proof = RangeProof::new(
            vec![
                source_new_balance,
                transfer_amount_lo as u64,
                transfer_amount_hi as u64,
            ],
            vec![64, 32, 32],
            vec![&opening_source, opening_lo, opening_hi],
            transcript,
        );

        Self {
            commitment_new_source: pod_commitment_new_source,
            equality_proof: equality_proof.into(),
            validity_proof: validity_proof.into(),
            range_proof: range_proof.try_into().expect("range proof: length error"),
        }
    }

    pub fn verify(
        &self,
        ciphertext_lo: &TransferAmountEncryption,
        ciphertext_hi: &TransferAmountEncryption,
        transfer_pubkeys: &TransferPubkeys,
        new_spendable_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        transcript.append_commitment(b"commitment-new-source", &self.commitment_new_source);

        let commitment: PedersenCommitment = self.commitment_new_source.try_into()?;
        let equality_proof: EqualityProof = self.equality_proof.try_into()?;
        let aggregated_validity_proof: AggregatedValidityProof = self.validity_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // verify equality proof
        //
        // TODO: we can also consider verifying equality and range proof in a batch
        equality_proof.verify(
            &transfer_pubkeys.source,
            new_spendable_ciphertext,
            &commitment,
            transcript,
        )?;

        // verify validity proof
        aggregated_validity_proof.verify(
            (&transfer_pubkeys.dest, &transfer_pubkeys.auditor),
            (&ciphertext_lo.commitment, &ciphertext_hi.commitment),
            (&ciphertext_lo.dest, &ciphertext_hi.dest),
            (&ciphertext_lo.auditor, &ciphertext_hi.auditor),
            transcript,
        )?;

        // verify range proof
        let commitment_new_source = self.commitment_new_source.try_into()?;
        range_proof.verify(
            vec![
                &commitment_new_source,
                &ciphertext_lo.commitment,
                &ciphertext_hi.commitment,
            ],
            vec![64_usize, 32_usize, 32_usize],
            transcript,
        )?;

        Ok(())
    }
}

/// The ElGamal public keys needed for a transfer
#[derive(Clone)]
#[repr(C)]
#[cfg(not(target_arch = "bpf"))]
pub struct TransferPubkeys {
    pub source: ElGamalPubkey,
    pub dest: ElGamalPubkey,
    pub auditor: ElGamalPubkey,
}

#[cfg(not(target_arch = "bpf"))]
impl TransferPubkeys {
    // TODO: use constructor instead
    pub fn to_bytes(&self) -> [u8; 96] {
        let mut bytes = [0u8; 96];
        bytes[..32].copy_from_slice(&self.source.to_bytes());
        bytes[32..64].copy_from_slice(&self.dest.to_bytes());
        bytes[64..96].copy_from_slice(&self.auditor.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProofError> {
        let bytes = array_ref![bytes, 0, 96];
        let (source, dest, auditor) = array_refs![bytes, 32, 32, 32];

        let source = ElGamalPubkey::from_bytes(source).ok_or(ProofError::Verification)?;
        let dest = ElGamalPubkey::from_bytes(dest).ok_or(ProofError::Verification)?;
        let auditor = ElGamalPubkey::from_bytes(auditor).ok_or(ProofError::Verification)?;

        Ok(Self {
            source,
            dest,
            auditor,
        })
    }
}

#[cfg(not(target_arch = "bpf"))]
impl pod::TransferPubkeys {
    pub fn new(source: &ElGamalPubkey, dest: &ElGamalPubkey, auditor: &ElGamalPubkey) -> Self {
        let mut bytes = [0u8; 96];
        bytes[..32].copy_from_slice(&source.to_bytes());
        bytes[32..64].copy_from_slice(&dest.to_bytes());
        bytes[64..96].copy_from_slice(&auditor.to_bytes());
        Self(bytes)
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::elgamal::ElGamalKeypair};

    #[test]
    fn test_transfer_correctness() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let source_keypair = ElGamalKeypair::new_rand();
        let dest_pk = ElGamalKeypair::new_rand().public;
        let auditor_pk = ElGamalKeypair::new_rand().public;

        // create source account spendable ciphertext
        let spendable_balance: u64 = 77;
        let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 55;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&dest_pk, &auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify().is_ok());
    }

    #[test]
    fn test_source_dest_ciphertext() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let source_keypair = ElGamalKeypair::new_rand();

        let ElGamalKeypair {
            public: dest_pk,
            secret: dest_sk,
        } = ElGamalKeypair::new_rand();

        let ElGamalKeypair {
            public: auditor_pk,
            secret: auditor_sk,
        } = ElGamalKeypair::new_rand();

        // create source account spendable ciphertext
        let spendable_balance: u64 = 77;
        let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 55;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&dest_pk, &auditor_pk),
        )
        .unwrap();

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Source, &source_keypair.secret)
                .unwrap(),
            55_u64,
        );

        assert_eq!(
            transfer_data.decrypt_amount(Role::Dest, &dest_sk).unwrap(),
            55_u64,
        );

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Auditor, &auditor_sk)
                .unwrap(),
            55_u64,
        );
    }
}
