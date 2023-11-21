#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey, ElGamalSecretKey},
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        instruction::transfer::{
            combine_lo_hi_ciphertexts, encryption::TransferAmountCiphertext, split_u64, Role,
        },
        range_proof::RangeProof,
        sigma_proofs::{
            batched_grouped_ciphertext_validity_proof::BatchedGroupedCiphertext2HandlesValidityProof,
            ciphertext_commitment_equality_proof::CiphertextCommitmentEqualityProof,
        },
        transcript::TranscriptProtocol,
    },
    bytemuck::bytes_of,
    merlin::Transcript,
    std::convert::TryInto,
};
use {
    crate::{
        instruction::{ProofType, ZkProofData},
        zk_token_elgamal::pod,
    },
    bytemuck::{Pod, Zeroable},
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

/// The instruction data that is needed for the `ProofInstruction::VerifyTransfer` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferData {
    /// The context data for the transfer proof
    pub context: TransferProofContext,

    /// Zero-knowledge proofs for Transfer
    pub proof: TransferProof,
}

/// The context data needed to verify a transfer proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferProofContext {
    /// Group encryption of the low 16 bits of the transfer amount
    pub ciphertext_lo: pod::TransferAmountCiphertext, // 128 bytes

    /// Group encryption of the high 48 bits of the transfer amount
    pub ciphertext_hi: pod::TransferAmountCiphertext, // 128 bytes

    /// The public encryption keys associated with the transfer: source, dest, and auditor
    pub transfer_pubkeys: TransferPubkeys, // 96 bytes

    /// The final spendable ciphertext after the transfer
    pub new_source_ciphertext: pod::ElGamalCiphertext, // 64 bytes
}

/// The ElGamal public keys needed for a transfer
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferPubkeys {
    pub source: pod::ElGamalPubkey,
    pub destination: pod::ElGamalPubkey,
    pub auditor: pod::ElGamalPubkey,
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

        let (ciphertext_lo, opening_lo) = TransferAmountCiphertext::new(
            amount_lo,
            source_keypair.pubkey(),
            destination_pubkey,
            auditor_pubkey,
        );

        let (ciphertext_hi, opening_hi) = TransferAmountCiphertext::new(
            amount_hi,
            source_keypair.pubkey(),
            destination_pubkey,
            auditor_pubkey,
        );

        // subtract transfer amount from the spendable ciphertext
        let new_spendable_balance = spendable_balance
            .checked_sub(transfer_amount)
            .ok_or(ProofError::Generation)?;

        let transfer_amount_lo_source = ElGamalCiphertext {
            commitment: *ciphertext_lo.get_commitment(),
            handle: *ciphertext_lo.get_source_handle(),
        };

        let transfer_amount_hi_source = ElGamalCiphertext {
            commitment: *ciphertext_hi.get_commitment(),
            handle: *ciphertext_hi.get_source_handle(),
        };

        let new_source_ciphertext = ciphertext_old_source
            - combine_lo_hi_ciphertexts(
                &transfer_amount_lo_source,
                &transfer_amount_hi_source,
                TRANSFER_AMOUNT_LO_BITS,
            );

        // generate transcript and append all public inputs
        let pod_transfer_pubkeys = TransferPubkeys {
            source: (*source_keypair.pubkey()).into(),
            destination: (*destination_pubkey).into(),
            auditor: (*auditor_pubkey).into(),
        };
        let pod_ciphertext_lo: pod::TransferAmountCiphertext = ciphertext_lo.into();
        let pod_ciphertext_hi: pod::TransferAmountCiphertext = ciphertext_hi.into();
        let pod_new_source_ciphertext: pod::ElGamalCiphertext = new_source_ciphertext.into();

        let context = TransferProofContext {
            ciphertext_lo: pod_ciphertext_lo,
            ciphertext_hi: pod_ciphertext_hi,
            transfer_pubkeys: pod_transfer_pubkeys,
            new_source_ciphertext: pod_new_source_ciphertext,
        };

        let mut transcript = context.new_transcript();
        let proof = TransferProof::new(
            (amount_lo, amount_hi),
            source_keypair,
            (destination_pubkey, auditor_pubkey),
            &opening_lo,
            &opening_hi,
            (new_spendable_balance, &new_source_ciphertext),
            &mut transcript,
        );

        Ok(Self { context, proof })
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_lo(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_lo: TransferAmountCiphertext = self.context.ciphertext_lo.try_into()?;

        let handle_lo = match role {
            Role::Source => Some(ciphertext_lo.get_source_handle()),
            Role::Destination => Some(ciphertext_lo.get_destination_handle()),
            Role::Auditor => Some(ciphertext_lo.get_auditor_handle()),
            Role::WithdrawWithheldAuthority => None,
        };

        if let Some(handle) = handle_lo {
            Ok(ElGamalCiphertext {
                commitment: *ciphertext_lo.get_commitment(),
                handle: *handle,
            })
        } else {
            Err(ProofError::MissingCiphertext)
        }
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_hi(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_hi: TransferAmountCiphertext = self.context.ciphertext_hi.try_into()?;

        let handle_hi = match role {
            Role::Source => Some(ciphertext_hi.get_source_handle()),
            Role::Destination => Some(ciphertext_hi.get_destination_handle()),
            Role::Auditor => Some(ciphertext_hi.get_auditor_handle()),
            Role::WithdrawWithheldAuthority => None,
        };

        if let Some(handle) = handle_hi {
            Ok(ElGamalCiphertext {
                commitment: *ciphertext_hi.get_commitment(),
                handle: *handle,
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

impl ZkProofData<TransferProofContext> for TransferData {
    const PROOF_TYPE: ProofType = ProofType::Transfer;

    fn context_data(&self) -> &TransferProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        // generate transcript and append all public inputs
        let mut transcript = self.context.new_transcript();

        let source_pubkey = self.context.transfer_pubkeys.source.try_into()?;
        let destination_pubkey = self.context.transfer_pubkeys.destination.try_into()?;
        let auditor_pubkey = self.context.transfer_pubkeys.auditor.try_into()?;

        let ciphertext_lo = self.context.ciphertext_lo.try_into()?;
        let ciphertext_hi = self.context.ciphertext_hi.try_into()?;
        let new_spendable_ciphertext = self.context.new_source_ciphertext.try_into()?;

        self.proof.verify(
            &source_pubkey,
            &destination_pubkey,
            &auditor_pubkey,
            &ciphertext_lo,
            &ciphertext_hi,
            &new_spendable_ciphertext,
            &mut transcript,
        )
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl TransferProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"transfer-proof");
        transcript.append_message(b"ciphertext-lo", bytes_of(&self.ciphertext_lo));
        transcript.append_message(b"ciphertext-hi", bytes_of(&self.ciphertext_hi));
        transcript.append_message(b"transfer-pubkeys", bytes_of(&self.transfer_pubkeys));
        transcript.append_message(
            b"new-source-ciphertext",
            bytes_of(&self.new_source_ciphertext),
        );
        transcript
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferProof {
    /// New Pedersen commitment for the remaining balance in source
    pub new_source_commitment: pod::PedersenCommitment,

    /// Associated equality proof
    pub equality_proof: pod::CiphertextCommitmentEqualityProof,

    /// Associated ciphertext validity proof
    pub validity_proof: pod::BatchedGroupedCiphertext2HandlesValidityProof,

    // Associated range proof
    pub range_proof: pod::RangeProofU128,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl TransferProof {
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
        let equality_proof = CiphertextCommitmentEqualityProof::new(
            source_keypair,
            new_source_ciphertext,
            &source_opening,
            source_new_balance,
            transcript,
        );

        // generate ciphertext validity proof
        let validity_proof = BatchedGroupedCiphertext2HandlesValidityProof::new(
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
            .expect("range proof: generator")
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
            .expect("range proof: generator")
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
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        ciphertext_lo: &TransferAmountCiphertext,
        ciphertext_hi: &TransferAmountCiphertext,
        ciphertext_new_spendable: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        transcript.append_commitment(b"commitment-new-source", &self.new_source_commitment);

        let commitment: PedersenCommitment = self.new_source_commitment.try_into()?;
        let equality_proof: CiphertextCommitmentEqualityProof = self.equality_proof.try_into()?;
        let aggregated_validity_proof: BatchedGroupedCiphertext2HandlesValidityProof =
            self.validity_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // verify equality proof
        equality_proof.verify(
            source_pubkey,
            ciphertext_new_spendable,
            &commitment,
            transcript,
        )?;

        // verify validity proof
        aggregated_validity_proof.verify(
            (destination_pubkey, auditor_pubkey),
            (
                ciphertext_lo.get_commitment(),
                ciphertext_hi.get_commitment(),
            ),
            (
                ciphertext_lo.get_destination_handle(),
                ciphertext_hi.get_destination_handle(),
            ),
            (
                ciphertext_lo.get_auditor_handle(),
                ciphertext_hi.get_auditor_handle(),
            ),
            transcript,
        )?;

        // verify range proof
        let new_source_commitment = self.new_source_commitment.try_into()?;
        if TRANSFER_AMOUNT_LO_BITS == 32 {
            range_proof.verify(
                vec![
                    &new_source_commitment,
                    ciphertext_lo.get_commitment(),
                    ciphertext_hi.get_commitment(),
                ],
                vec![
                    TRANSFER_SOURCE_AMOUNT_BITS,
                    TRANSFER_AMOUNT_LO_BITS,
                    TRANSFER_AMOUNT_HI_BITS,
                ],
                transcript,
            )?;
        } else {
            let commitment_lo_negated = &(*COMMITMENT_MAX) - ciphertext_lo.get_commitment();

            range_proof.verify(
                vec![
                    &new_source_commitment,
                    ciphertext_lo.get_commitment(),
                    &commitment_lo_negated,
                    ciphertext_hi.get_commitment(),
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

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::elgamal::ElGamalKeypair};

    #[test]
    fn test_transfer_correctness() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let source_keypair = ElGamalKeypair::new_rand();

        let dest_keypair = ElGamalKeypair::new_rand();
        let dest_pk = dest_keypair.pubkey();

        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pk = auditor_keypair.pubkey();

        // Case 1: transfer 0 amount

        // create source account spendable ciphertext
        let spendable_balance: u64 = 0;
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 0;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (dest_pk, auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify_proof().is_ok());

        // Case 2: transfer max amount

        // create source account spendable ciphertext
        let spendable_balance: u64 = u64::max_value();
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 =
            (1u64 << (TRANSFER_AMOUNT_LO_BITS + TRANSFER_AMOUNT_HI_BITS)) - 1;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (dest_pk, auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify_proof().is_ok());

        // Case 3: general success case

        // create source account spendable ciphertext
        let spendable_balance: u64 = 77;
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 55;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (dest_pk, auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify_proof().is_ok());

        // Case 4: destination pubkey is invalid
        let spendable_balance: u64 = 0;
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);
        let transfer_amount: u64 = 0;

        let dest_pk = pod::ElGamalPubkey::zeroed().try_into().unwrap();
        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pk = auditor_keypair.pubkey();

        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&dest_pk, auditor_pk),
        )
        .unwrap();

        assert!(transfer_data.verify_proof().is_err());
    }

    #[test]
    fn test_source_dest_ciphertext() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let source_keypair = ElGamalKeypair::new_rand();
        let dest_pk = source_keypair.pubkey();
        let dest_sk = source_keypair.secret();

        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pk = auditor_keypair.pubkey();
        let auditor_sk = auditor_keypair.secret();

        // create source account spendable ciphertext
        let spendable_balance: u64 = 770000;
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 550000;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (dest_pk, auditor_pk),
        )
        .unwrap();

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Source, source_keypair.secret())
                .unwrap(),
            550000_u64,
        );

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Destination, dest_sk)
                .unwrap(),
            550000_u64,
        );

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Auditor, auditor_sk)
                .unwrap(),
            550000_u64,
        );
    }
}
