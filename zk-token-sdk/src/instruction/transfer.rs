use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{
                DecryptHandle, ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey, ElGamalSecretKey,
            },
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        instruction::{combine_lo_hi_ciphertexts, split_u64, Role, Verifiable},
        range_proof::RangeProof,
        sigma_proofs::{
            equality_proof::CtxtCommEqualityProof, validity_proof::AggregatedValidityProof,
        },
        transcript::TranscriptProtocol,
    },
    arrayref::{array_ref, array_refs},
    merlin::Transcript,
    std::convert::TryInto,
};

#[cfg(not(target_os = "solana"))]
const TRANSFER_SOURCE_AMOUNT_BITS: usize = 64;
#[cfg(not(target_os = "solana"))]
const TRANSFER_AMOUNT_LO_BITS: usize = 16;
#[cfg(not(target_os = "solana"))]
const TRANSFER_AMOUNT_LO_NEGATED_BITS: usize = 16;
#[cfg(not(target_os = "solana"))]
const TRANSFER_AMOUNT_HI_BITS: usize = 32;

#[cfg(not(target_os = "solana"))]
lazy_static::lazy_static! {
    pub static ref COMMITMENT_MAX: PedersenCommitment = Pedersen::encode((1_u64 <<
                                                                         TRANSFER_AMOUNT_LO_NEGATED_BITS) - 1);
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferData {
    /// Group encryption of the low 16 bits of the transfer amount
    pub ciphertext_lo: pod::TransferAmountEncryption,

    /// Group encryption of the high 48 bits of the transfer amount
    pub ciphertext_hi: pod::TransferAmountEncryption,

    /// The public encryption keys associated with the transfer: source, dest, and auditor
    pub transfer_pubkeys: pod::TransferPubkeys,

    /// The final spendable ciphertext after the transfer
    pub new_source_ciphertext: pod::ElGamalCiphertext,

    /// Zero-knowledge proofs for Transfer
    pub proof: TransferProof,
}

#[cfg(not(target_os = "solana"))]
impl TransferData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transfer_amount: u64,
        (spendable_balance, ciphertext_old_source): (u64, &ElGamalCiphertext),
        source_keypair: &ElGamalKeypair,
        (destination_pubkey, auditor_pubkey): (&ElGamalPubkey, &ElGamalPubkey),
    ) -> Result<Self, ProofError> {
        // split and encrypt transfer amount
        let (amount_lo, amount_hi) = split_u64(transfer_amount, TRANSFER_AMOUNT_LO_BITS);

        let (ciphertext_lo, opening_lo) = TransferAmountEncryption::new(
            amount_lo,
            &source_keypair.public,
            destination_pubkey,
            auditor_pubkey,
        );

        let (ciphertext_hi, opening_hi) = TransferAmountEncryption::new(
            amount_hi,
            &source_keypair.public,
            destination_pubkey,
            auditor_pubkey,
        );

        // subtract transfer amount from the spendable ciphertext
        let new_spendable_balance = spendable_balance
            .checked_sub(transfer_amount)
            .ok_or(ProofError::Generation)?;

        let transfer_amount_lo_source = ElGamalCiphertext {
            commitment: ciphertext_lo.commitment,
            handle: ciphertext_lo.source_handle,
        };

        let transfer_amount_hi_source = ElGamalCiphertext {
            commitment: ciphertext_hi.commitment,
            handle: ciphertext_hi.source_handle,
        };

        let new_source_ciphertext = ciphertext_old_source
            - combine_lo_hi_ciphertexts(
                &transfer_amount_lo_source,
                &transfer_amount_hi_source,
                TRANSFER_AMOUNT_LO_BITS,
            );

        // generate transcript and append all public inputs
        let pod_transfer_pubkeys = pod::TransferPubkeys {
            source_pubkey: source_keypair.public.into(),
            destination_pubkey: (*destination_pubkey).into(),
            auditor_pubkey: (*auditor_pubkey).into(),
        };
        let pod_ciphertext_lo: pod::TransferAmountEncryption = ciphertext_lo.into();
        let pod_ciphertext_hi: pod::TransferAmountEncryption = ciphertext_hi.into();
        let pod_new_source_ciphertext: pod::ElGamalCiphertext = new_source_ciphertext.into();

        let mut transcript = TransferProof::transcript_new(
            &pod_transfer_pubkeys,
            &pod_ciphertext_lo,
            &pod_ciphertext_hi,
            &pod_new_source_ciphertext,
        );

        let proof = TransferProof::new(
            (amount_lo, amount_hi),
            source_keypair,
            (destination_pubkey, auditor_pubkey),
            &opening_lo,
            &opening_hi,
            (new_spendable_balance, &new_source_ciphertext),
            &mut transcript,
        );

        Ok(Self {
            ciphertext_lo: pod_ciphertext_lo,
            ciphertext_hi: pod_ciphertext_hi,
            transfer_pubkeys: pod_transfer_pubkeys,
            new_source_ciphertext: pod_new_source_ciphertext,
            proof,
        })
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_lo(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_lo: TransferAmountEncryption = self.ciphertext_lo.try_into()?;

        let handle_lo = match role {
            Role::Source => Some(ciphertext_lo.source_handle),
            Role::Destination => Some(ciphertext_lo.destination_handle),
            Role::Auditor => Some(ciphertext_lo.auditor_handle),
            Role::WithdrawWithheldAuthority => None,
        };

        if let Some(handle) = handle_lo {
            Ok(ElGamalCiphertext {
                commitment: ciphertext_lo.commitment,
                handle,
            })
        } else {
            Err(ProofError::MissingCiphertext)
        }
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_hi(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_hi: TransferAmountEncryption = self.ciphertext_hi.try_into()?;

        let handle_hi = match role {
            Role::Source => Some(ciphertext_hi.source_handle),
            Role::Destination => Some(ciphertext_hi.destination_handle),
            Role::Auditor => Some(ciphertext_hi.auditor_handle),
            Role::WithdrawWithheldAuthority => None,
        };

        if let Some(handle) = handle_hi {
            Ok(ElGamalCiphertext {
                commitment: ciphertext_hi.commitment,
                handle,
            })
        } else {
            Err(ProofError::MissingCiphertext)
        }
    }

    /// Decrypts transfer amount from transfer data
    pub fn decrypt_amount(&self, role: Role, sk: &ElGamalSecretKey) -> Result<u64, ProofError> {
        let ciphertext_lo = self.ciphertext_lo(role)?;
        let ciphertext_hi = self.ciphertext_hi(role)?;

        let amount_lo = ciphertext_lo.decrypt_u32(sk);
        let amount_hi = ciphertext_hi.decrypt_u32(sk);

        if let (Some(amount_lo), Some(amount_hi)) = (amount_lo, amount_hi) {
            let two_power = 1 << TRANSFER_AMOUNT_LO_BITS;
            Ok(amount_lo + two_power * amount_hi)
        } else {
            Err(ProofError::Decryption)
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl Verifiable for TransferData {
    fn verify(&self) -> Result<(), ProofError> {
        // generate transcript and append all public inputs
        let mut transcript = TransferProof::transcript_new(
            &self.transfer_pubkeys,
            &self.ciphertext_lo,
            &self.ciphertext_hi,
            &self.new_source_ciphertext,
        );

        let ciphertext_lo = self.ciphertext_lo.try_into()?;
        let ciphertext_hi = self.ciphertext_hi.try_into()?;
        let transfer_pubkeys = self.transfer_pubkeys.try_into()?;
        let new_spendable_ciphertext = self.new_source_ciphertext.try_into()?;

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
    pub new_source_commitment: pod::PedersenCommitment,

    /// Associated equality proof
    pub equality_proof: pod::CtxtCommEqualityProof,

    /// Associated ciphertext validity proof
    pub validity_proof: pod::AggregatedValidityProof,

    // Associated range proof
    pub range_proof: pod::RangeProof128,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl TransferProof {
    fn transcript_new(
        transfer_pubkeys: &pod::TransferPubkeys,
        ciphertext_lo: &pod::TransferAmountEncryption,
        ciphertext_hi: &pod::TransferAmountEncryption,
        new_source_ciphertext: &pod::ElGamalCiphertext,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"transfer-proof");

        transcript.append_pubkey(b"pubkey-source", &transfer_pubkeys.source_pubkey);
        transcript.append_pubkey(b"pubkey-dest", &transfer_pubkeys.destination_pubkey);
        transcript.append_pubkey(b"pubkey-auditor", &transfer_pubkeys.auditor_pubkey);

        transcript.append_commitment(b"comm-lo-amount", &ciphertext_lo.commitment);
        transcript.append_handle(b"handle-lo-source", &ciphertext_lo.source_handle);
        transcript.append_handle(b"handle-lo-dest", &ciphertext_lo.destination_handle);
        transcript.append_handle(b"handle-lo-auditor", &ciphertext_lo.auditor_handle);

        transcript.append_commitment(b"comm-hi-amount", &ciphertext_hi.commitment);
        transcript.append_handle(b"handle-hi-source", &ciphertext_hi.source_handle);
        transcript.append_handle(b"handle-hi-dest", &ciphertext_hi.destination_handle);
        transcript.append_handle(b"handle-hi-auditor", &ciphertext_hi.auditor_handle);

        transcript.append_ciphertext(b"ciphertext-new-source", new_source_ciphertext);

        transcript
    }

    pub fn new(
        (transfer_amount_lo, transfer_amount_hi): (u64, u64),
        source_keypair: &ElGamalKeypair,
        (destination_pubkey, auditor_pubkey): (&ElGamalPubkey, &ElGamalPubkey),
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
        (source_new_balance, new_source_ciphertext): (u64, &ElGamalCiphertext),
        transcript: &mut Transcript,
    ) -> Self {
        // generate a Pedersen commitment for the remaining balance in source
        let (new_source_commitment, source_opening) = Pedersen::new(source_new_balance);

        let pod_new_source_commitment: pod::PedersenCommitment = new_source_commitment.into();
        transcript.append_commitment(b"commitment-new-source", &pod_new_source_commitment);

        // generate equality_proof
        let equality_proof = CtxtCommEqualityProof::new(
            source_keypair,
            new_source_ciphertext,
            source_new_balance,
            &source_opening,
            transcript,
        );

        // generate ciphertext validity proof
        let validity_proof = AggregatedValidityProof::new(
            (destination_pubkey, auditor_pubkey),
            (transfer_amount_lo, transfer_amount_hi),
            (opening_lo, opening_hi),
            transcript,
        );

        // generate the range proof
        let range_proof = if TRANSFER_AMOUNT_LO_BITS == 32 {
            RangeProof::new(
                vec![source_new_balance, transfer_amount_lo, transfer_amount_hi],
                vec![
                    TRANSFER_SOURCE_AMOUNT_BITS,
                    TRANSFER_AMOUNT_LO_BITS,
                    TRANSFER_AMOUNT_HI_BITS,
                ],
                vec![&source_opening, opening_lo, opening_hi],
                transcript,
            )
        } else {
            let transfer_amount_lo_negated =
                (1 << TRANSFER_AMOUNT_LO_NEGATED_BITS) - 1 - transfer_amount_lo;
            let opening_lo_negated = &PedersenOpening::default() - opening_lo;

            RangeProof::new(
                vec![
                    source_new_balance,
                    transfer_amount_lo,
                    transfer_amount_lo_negated,
                    transfer_amount_hi,
                ],
                vec![
                    TRANSFER_SOURCE_AMOUNT_BITS,
                    TRANSFER_AMOUNT_LO_BITS,
                    TRANSFER_AMOUNT_LO_NEGATED_BITS,
                    TRANSFER_AMOUNT_HI_BITS,
                ],
                vec![&source_opening, opening_lo, &opening_lo_negated, opening_hi],
                transcript,
            )
        };

        Self {
            new_source_commitment: pod_new_source_commitment,
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
        ciphertext_new_spendable: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        transcript.append_commitment(b"commitment-new-source", &self.new_source_commitment);

        let commitment: PedersenCommitment = self.new_source_commitment.try_into()?;
        let equality_proof: CtxtCommEqualityProof = self.equality_proof.try_into()?;
        let aggregated_validity_proof: AggregatedValidityProof = self.validity_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // verify equality proof
        //
        // TODO: we can also consider verifying equality and range proof in a batch
        equality_proof.verify(
            &transfer_pubkeys.source_pubkey,
            ciphertext_new_spendable,
            &commitment,
            transcript,
        )?;

        // verify validity proof
        aggregated_validity_proof.verify(
            (
                &transfer_pubkeys.destination_pubkey,
                &transfer_pubkeys.auditor_pubkey,
            ),
            (&ciphertext_lo.commitment, &ciphertext_hi.commitment),
            (
                &ciphertext_lo.destination_handle,
                &ciphertext_hi.destination_handle,
            ),
            (&ciphertext_lo.auditor_handle, &ciphertext_hi.auditor_handle),
            transcript,
        )?;

        // verify range proof
        let new_source_commitment = self.new_source_commitment.try_into()?;
        if TRANSFER_AMOUNT_LO_BITS == 32 {
            range_proof.verify(
                vec![
                    &new_source_commitment,
                    &ciphertext_lo.commitment,
                    &ciphertext_hi.commitment,
                ],
                vec![
                    TRANSFER_SOURCE_AMOUNT_BITS,
                    TRANSFER_AMOUNT_LO_BITS,
                    TRANSFER_AMOUNT_HI_BITS,
                ],
                transcript,
            )?;
        } else {
            let commitment_lo_negated = &(*COMMITMENT_MAX) - &ciphertext_lo.commitment;

            range_proof.verify(
                vec![
                    &new_source_commitment,
                    &ciphertext_lo.commitment,
                    &commitment_lo_negated,
                    &ciphertext_hi.commitment,
                ],
                vec![
                    TRANSFER_SOURCE_AMOUNT_BITS,
                    TRANSFER_AMOUNT_LO_BITS,
                    TRANSFER_AMOUNT_LO_NEGATED_BITS,
                    TRANSFER_AMOUNT_HI_BITS,
                ],
                transcript,
            )?;
        }

        Ok(())
    }
}

/// The ElGamal public keys needed for a transfer
#[derive(Clone)]
#[repr(C)]
#[cfg(not(target_os = "solana"))]
pub struct TransferPubkeys {
    pub source_pubkey: ElGamalPubkey,
    pub destination_pubkey: ElGamalPubkey,
    pub auditor_pubkey: ElGamalPubkey,
}

#[cfg(not(target_os = "solana"))]
impl TransferPubkeys {
    // TODO: use constructor instead
    pub fn to_bytes(&self) -> [u8; 96] {
        let mut bytes = [0u8; 96];
        bytes[..32].copy_from_slice(&self.source_pubkey.to_bytes());
        bytes[32..64].copy_from_slice(&self.destination_pubkey.to_bytes());
        bytes[64..96].copy_from_slice(&self.auditor_pubkey.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProofError> {
        let bytes = array_ref![bytes, 0, 96];
        let (source_pubkey, destination_pubkey, auditor_pubkey) = array_refs![bytes, 32, 32, 32];

        let source_pubkey =
            ElGamalPubkey::from_bytes(source_pubkey).ok_or(ProofError::PubkeyDeserialization)?;
        let destination_pubkey = ElGamalPubkey::from_bytes(destination_pubkey)
            .ok_or(ProofError::PubkeyDeserialization)?;
        let auditor_pubkey =
            ElGamalPubkey::from_bytes(auditor_pubkey).ok_or(ProofError::PubkeyDeserialization)?;

        Ok(Self {
            source_pubkey,
            destination_pubkey,
            auditor_pubkey,
        })
    }
}

#[derive(Clone)]
#[repr(C)]
#[cfg(not(target_os = "solana"))]
pub struct TransferAmountEncryption {
    pub commitment: PedersenCommitment,
    pub source_handle: DecryptHandle,
    pub destination_handle: DecryptHandle,
    pub auditor_handle: DecryptHandle,
}

#[cfg(not(target_os = "solana"))]
impl TransferAmountEncryption {
    pub fn new(
        amount: u64,
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
    ) -> (Self, PedersenOpening) {
        let (commitment, opening) = Pedersen::new(amount);
        let transfer_amount_encryption = Self {
            commitment,
            source_handle: source_pubkey.decrypt_handle(&opening),
            destination_handle: destination_pubkey.decrypt_handle(&opening),
            auditor_handle: auditor_pubkey.decrypt_handle(&opening),
        };

        (transfer_amount_encryption, opening)
    }

    pub fn to_pod(&self) -> pod::TransferAmountEncryption {
        pod::TransferAmountEncryption {
            commitment: self.commitment.into(),
            source_handle: self.source_handle.into(),
            destination_handle: self.destination_handle.into(),
            auditor_handle: self.auditor_handle.into(),
        }
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

        // Case 1: transfer 0 amount

        // create source account spendable ciphertext
        let spendable_balance: u64 = 0;
        let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 0;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&dest_pk, &auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify().is_ok());

        // Case 2: transfer max amount

        // create source account spendable ciphertext
        let spendable_balance: u64 = u64::max_value();
        let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 =
            (1u64 << (TRANSFER_AMOUNT_LO_BITS + TRANSFER_AMOUNT_HI_BITS)) - 1;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&dest_pk, &auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify().is_ok());

        // Case 3: general success case

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

        // Case 4: invalid destination or auditor pubkey
        let spendable_balance: u64 = 0;
        let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

        let transfer_amount: u64 = 0;

        // destination pubkey invalid
        let dest_pk = pod::ElGamalPubkey::zeroed().try_into().unwrap();
        let auditor_pk = ElGamalKeypair::new_rand().public;

        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&dest_pk, &auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify().is_err());

        // auditor pubkey invalid
        let dest_pk = ElGamalKeypair::new_rand().public;
        let auditor_pk = pod::ElGamalPubkey::zeroed().try_into().unwrap();

        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&dest_pk, &auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify().is_err());
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
        let spendable_balance: u64 = 770000;
        let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 550000;

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
            550000_u64,
        );

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Destination, &dest_sk)
                .unwrap(),
            550000_u64,
        );

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Auditor, &auditor_sk)
                .unwrap(),
            550000_u64,
        );
    }
}
