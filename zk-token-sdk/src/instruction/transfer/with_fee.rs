#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey, ElGamalSecretKey},
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        instruction::transfer::{
            combine_lo_hi_ciphertexts, combine_lo_hi_commitments, combine_lo_hi_openings,
            combine_lo_hi_u64,
            encryption::{FeeEncryption, TransferAmountCiphertext},
            split_u64, FeeParameters, Role,
        },
        range_proof::RangeProof,
        sigma_proofs::{
            batched_grouped_ciphertext_validity_proof::BatchedGroupedCiphertext2HandlesValidityProof,
            ciphertext_commitment_equality_proof::CiphertextCommitmentEqualityProof,
            fee_proof::FeeSigmaProof,
        },
        transcript::TranscriptProtocol,
    },
    bytemuck::bytes_of,
    curve25519_dalek::scalar::Scalar,
    merlin::Transcript,
    std::convert::TryInto,
    subtle::{ConditionallySelectable, ConstantTimeGreater},
};
use {
    crate::{
        instruction::{ProofType, ZkProofData},
        zk_token_elgamal::pod,
    },
    bytemuck::{Pod, Zeroable},
};

#[cfg(not(target_os = "solana"))]
const MAX_FEE_BASIS_POINTS: u64 = 10_000;
#[cfg(not(target_os = "solana"))]
const ONE_IN_BASIS_POINTS: u128 = MAX_FEE_BASIS_POINTS as u128;

#[cfg(not(target_os = "solana"))]
const TRANSFER_SOURCE_AMOUNT_BITS: usize = 64;
#[cfg(not(target_os = "solana"))]
const TRANSFER_AMOUNT_LO_BITS: usize = 16;
#[cfg(not(target_os = "solana"))]
const TRANSFER_AMOUNT_LO_NEGATED_BITS: usize = 16;
#[cfg(not(target_os = "solana"))]
const TRANSFER_AMOUNT_HI_BITS: usize = 32;
#[cfg(not(target_os = "solana"))]
const TRANSFER_DELTA_BITS: usize = 48;
#[cfg(not(target_os = "solana"))]
const FEE_AMOUNT_LO_BITS: usize = 16;
#[cfg(not(target_os = "solana"))]
const FEE_AMOUNT_HI_BITS: usize = 32;

#[cfg(not(target_os = "solana"))]
lazy_static::lazy_static! {
    pub static ref COMMITMENT_MAX: PedersenCommitment = Pedersen::encode((1_u64 <<
                                                                         TRANSFER_AMOUNT_LO_NEGATED_BITS) - 1);
    pub static ref COMMITMENT_MAX_FEE_BASIS_POINTS: PedersenCommitment = Pedersen::encode(MAX_FEE_BASIS_POINTS);
}

/// The instruction data that is needed for the `ProofInstruction::TransferWithFee` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferWithFeeData {
    /// The context data for the transfer with fee proof
    pub context: TransferWithFeeProofContext,

    // transfer fee proof
    pub proof: TransferWithFeeProof,
}

/// The context data needed to verify a transfer-with-fee proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferWithFeeProofContext {
    /// Group encryption of the low 16 bites of the transfer amount
    pub ciphertext_lo: pod::TransferAmountCiphertext, // 128 bytes

    /// Group encryption of the high 48 bits of the transfer amount
    pub ciphertext_hi: pod::TransferAmountCiphertext, // 128 bytes

    /// The public encryption keys associated with the transfer: source, dest, and auditor
    pub transfer_with_fee_pubkeys: TransferWithFeePubkeys, // 128 bytes

    /// The final spendable ciphertext after the transfer,
    pub new_source_ciphertext: pod::ElGamalCiphertext, // 64 bytes

    // transfer fee encryption of the low 16 bits of the transfer fee amount
    pub fee_ciphertext_lo: pod::FeeEncryption, // 96 bytes

    // transfer fee encryption of the hi 32 bits of the transfer fee amount
    pub fee_ciphertext_hi: pod::FeeEncryption, // 96 bytes

    // fee parameters
    pub fee_parameters: pod::FeeParameters, // 10 bytes
}

/// The ElGamal public keys needed for a transfer with fee
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferWithFeePubkeys {
    pub source: pod::ElGamalPubkey,
    pub destination: pod::ElGamalPubkey,
    pub auditor: pod::ElGamalPubkey,
    pub withdraw_withheld_authority: pod::ElGamalPubkey,
}

#[cfg(not(target_os = "solana"))]
impl TransferWithFeeData {
    pub fn new(
        transfer_amount: u64,
        (spendable_balance, old_source_ciphertext): (u64, &ElGamalCiphertext),
        source_keypair: &ElGamalKeypair,
        (destination_pubkey, auditor_pubkey): (&ElGamalPubkey, &ElGamalPubkey),
        fee_parameters: FeeParameters,
        withdraw_withheld_authority_pubkey: &ElGamalPubkey,
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

        let new_source_ciphertext = old_source_ciphertext
            - combine_lo_hi_ciphertexts(
                &transfer_amount_lo_source,
                &transfer_amount_hi_source,
                TRANSFER_AMOUNT_LO_BITS,
            );

        // calculate fee
        //
        // TODO: add comment on delta fee
        let (fee_amount, delta_fee) =
            calculate_fee(transfer_amount, fee_parameters.fee_rate_basis_points)
                .ok_or(ProofError::Generation)?;

        let below_max = u64::ct_gt(&fee_parameters.maximum_fee, &fee_amount);
        let fee_to_encrypt =
            u64::conditional_select(&fee_parameters.maximum_fee, &fee_amount, below_max);

        // split and encrypt fee
        let (fee_to_encrypt_lo, fee_to_encrypt_hi) = split_u64(fee_to_encrypt, FEE_AMOUNT_LO_BITS);

        let (fee_ciphertext_lo, opening_fee_lo) = FeeEncryption::new(
            fee_to_encrypt_lo,
            destination_pubkey,
            withdraw_withheld_authority_pubkey,
        );

        let (fee_ciphertext_hi, opening_fee_hi) = FeeEncryption::new(
            fee_to_encrypt_hi,
            destination_pubkey,
            withdraw_withheld_authority_pubkey,
        );

        // generate transcript and append all public inputs
        let pod_transfer_with_fee_pubkeys = TransferWithFeePubkeys {
            source: (*source_keypair.pubkey()).into(),
            destination: (*destination_pubkey).into(),
            auditor: (*auditor_pubkey).into(),
            withdraw_withheld_authority: (*withdraw_withheld_authority_pubkey).into(),
        };
        let pod_ciphertext_lo: pod::TransferAmountCiphertext = ciphertext_lo.into();
        let pod_ciphertext_hi: pod::TransferAmountCiphertext = ciphertext_hi.into();
        let pod_new_source_ciphertext: pod::ElGamalCiphertext = new_source_ciphertext.into();
        let pod_fee_ciphertext_lo: pod::FeeEncryption = fee_ciphertext_lo.into();
        let pod_fee_ciphertext_hi: pod::FeeEncryption = fee_ciphertext_hi.into();

        let context = TransferWithFeeProofContext {
            ciphertext_lo: pod_ciphertext_lo,
            ciphertext_hi: pod_ciphertext_hi,
            transfer_with_fee_pubkeys: pod_transfer_with_fee_pubkeys,
            new_source_ciphertext: pod_new_source_ciphertext,
            fee_ciphertext_lo: pod_fee_ciphertext_lo,
            fee_ciphertext_hi: pod_fee_ciphertext_hi,
            fee_parameters: fee_parameters.into(),
        };

        let mut transcript = context.new_transcript();

        let proof = TransferWithFeeProof::new(
            (amount_lo, &ciphertext_lo, &opening_lo),
            (amount_hi, &ciphertext_hi, &opening_hi),
            source_keypair,
            (destination_pubkey, auditor_pubkey),
            (new_spendable_balance, &new_source_ciphertext),
            (fee_to_encrypt_lo, &fee_ciphertext_lo, &opening_fee_lo),
            (fee_to_encrypt_hi, &fee_ciphertext_hi, &opening_fee_hi),
            delta_fee,
            withdraw_withheld_authority_pubkey,
            fee_parameters,
            &mut transcript,
        );

        Ok(Self { context, proof })
    }

    /// Extracts the lo ciphertexts associated with a transfer-with-fee data
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

    /// Extracts the lo ciphertexts associated with a transfer-with-fee data
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

    /// Extracts the lo fee ciphertexts associated with a transfer_with_fee data
    fn fee_ciphertext_lo(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let fee_ciphertext_lo: FeeEncryption = self.context.fee_ciphertext_lo.try_into()?;

        let fee_handle_lo = match role {
            Role::Source => None,
            Role::Destination => Some(fee_ciphertext_lo.get_destination_handle()),
            Role::Auditor => None,
            Role::WithdrawWithheldAuthority => {
                Some(fee_ciphertext_lo.get_withdraw_withheld_authority_handle())
            }
        };

        if let Some(handle) = fee_handle_lo {
            Ok(ElGamalCiphertext {
                commitment: *fee_ciphertext_lo.get_commitment(),
                handle: *handle,
            })
        } else {
            Err(ProofError::MissingCiphertext)
        }
    }

    /// Extracts the hi fee ciphertexts associated with a transfer_with_fee data
    fn fee_ciphertext_hi(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let fee_ciphertext_hi: FeeEncryption = self.context.fee_ciphertext_hi.try_into()?;

        let fee_handle_hi = match role {
            Role::Source => None,
            Role::Destination => Some(fee_ciphertext_hi.get_destination_handle()),
            Role::Auditor => None,
            Role::WithdrawWithheldAuthority => {
                Some(fee_ciphertext_hi.get_withdraw_withheld_authority_handle())
            }
        };

        if let Some(handle) = fee_handle_hi {
            Ok(ElGamalCiphertext {
                commitment: *fee_ciphertext_hi.get_commitment(),
                handle: *handle,
            })
        } else {
            Err(ProofError::MissingCiphertext)
        }
    }

    /// Decrypts transfer amount from transfer-with-fee data
    pub fn decrypt_amount(&self, role: Role, sk: &ElGamalSecretKey) -> Result<u64, ProofError> {
        let ciphertext_lo = self.ciphertext_lo(role)?;
        let ciphertext_hi = self.ciphertext_hi(role)?;

        let amount_lo = ciphertext_lo.decrypt_u32(sk);
        let amount_hi = ciphertext_hi.decrypt_u32(sk);

        if let (Some(amount_lo), Some(amount_hi)) = (amount_lo, amount_hi) {
            let shifted_amount_hi = amount_hi << TRANSFER_AMOUNT_LO_BITS;
            Ok(amount_lo + shifted_amount_hi)
        } else {
            Err(ProofError::Decryption)
        }
    }

    /// Decrypts transfer amount from transfer-with-fee data
    pub fn decrypt_fee_amount(&self, role: Role, sk: &ElGamalSecretKey) -> Result<u64, ProofError> {
        let ciphertext_lo = self.fee_ciphertext_lo(role)?;
        let ciphertext_hi = self.fee_ciphertext_hi(role)?;

        let fee_amount_lo = ciphertext_lo.decrypt_u32(sk);
        let fee_amount_hi = ciphertext_hi.decrypt_u32(sk);

        if let (Some(fee_amount_lo), Some(fee_amount_hi)) = (fee_amount_lo, fee_amount_hi) {
            let shifted_fee_amount_hi = fee_amount_hi << FEE_AMOUNT_LO_BITS;
            Ok(fee_amount_lo + shifted_fee_amount_hi)
        } else {
            Err(ProofError::Decryption)
        }
    }
}

impl ZkProofData<TransferWithFeeProofContext> for TransferWithFeeData {
    const PROOF_TYPE: ProofType = ProofType::TransferWithFee;

    fn context_data(&self) -> &TransferWithFeeProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context.new_transcript();

        let source_pubkey = self.context.transfer_with_fee_pubkeys.source.try_into()?;
        let destination_pubkey = self
            .context
            .transfer_with_fee_pubkeys
            .destination
            .try_into()?;
        let auditor_pubkey = self.context.transfer_with_fee_pubkeys.auditor.try_into()?;
        let withdraw_withheld_authority_pubkey = self
            .context
            .transfer_with_fee_pubkeys
            .withdraw_withheld_authority
            .try_into()?;

        let ciphertext_lo = self.context.ciphertext_lo.try_into()?;
        let ciphertext_hi = self.context.ciphertext_hi.try_into()?;
        let new_source_ciphertext = self.context.new_source_ciphertext.try_into()?;

        let fee_ciphertext_lo = self.context.fee_ciphertext_lo.try_into()?;
        let fee_ciphertext_hi = self.context.fee_ciphertext_hi.try_into()?;
        let fee_parameters = self.context.fee_parameters.into();

        self.proof.verify(
            &source_pubkey,
            &destination_pubkey,
            &auditor_pubkey,
            &withdraw_withheld_authority_pubkey,
            &ciphertext_lo,
            &ciphertext_hi,
            &new_source_ciphertext,
            &fee_ciphertext_lo,
            &fee_ciphertext_hi,
            fee_parameters,
            &mut transcript,
        )
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl TransferWithFeeProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"transfer-with-fee-proof");
        transcript.append_message(b"ciphertext-lo", bytes_of(&self.ciphertext_lo));
        transcript.append_message(b"ciphertext-hi", bytes_of(&self.ciphertext_hi));
        transcript.append_message(
            b"transfer-with-fee-pubkeys",
            bytes_of(&self.transfer_with_fee_pubkeys),
        );
        transcript.append_message(
            b"new-source-ciphertext",
            bytes_of(&self.new_source_ciphertext),
        );
        transcript.append_message(b"fee-ciphertext-lo", bytes_of(&self.fee_ciphertext_lo));
        transcript.append_message(b"fee-ciphertext-hi", bytes_of(&self.fee_ciphertext_hi));
        transcript.append_message(b"fee-parameters", bytes_of(&self.fee_parameters));
        transcript
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct TransferWithFeeProof {
    pub new_source_commitment: pod::PedersenCommitment,
    pub claimed_commitment: pod::PedersenCommitment,
    pub equality_proof: pod::CiphertextCommitmentEqualityProof,
    pub ciphertext_amount_validity_proof: pod::BatchedGroupedCiphertext2HandlesValidityProof,
    pub fee_sigma_proof: pod::FeeSigmaProof,
    pub fee_ciphertext_validity_proof: pod::BatchedGroupedCiphertext2HandlesValidityProof,
    pub range_proof: pod::RangeProofU256,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl TransferWithFeeProof {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::many_single_char_names)]
    pub fn new(
        transfer_amount_lo_data: (u64, &TransferAmountCiphertext, &PedersenOpening),
        transfer_amount_hi_data: (u64, &TransferAmountCiphertext, &PedersenOpening),
        source_keypair: &ElGamalKeypair,
        (destination_pubkey, auditor_pubkey): (&ElGamalPubkey, &ElGamalPubkey),
        (source_new_balance, new_source_ciphertext): (u64, &ElGamalCiphertext),
        // fee parameters
        (fee_amount_lo, fee_ciphertext_lo, opening_fee_lo): (u64, &FeeEncryption, &PedersenOpening),
        (fee_amount_hi, fee_ciphertext_hi, opening_fee_hi): (u64, &FeeEncryption, &PedersenOpening),
        delta_fee: u64,
        withdraw_withheld_authority_pubkey: &ElGamalPubkey,
        fee_parameters: FeeParameters,
        transcript: &mut Transcript,
    ) -> Self {
        let (transfer_amount_lo, ciphertext_lo, opening_lo) = transfer_amount_lo_data;
        let (transfer_amount_hi, ciphertext_hi, opening_hi) = transfer_amount_hi_data;

        // generate a Pedersen commitment for the remaining balance in source
        let (new_source_commitment, opening_source) = Pedersen::new(source_new_balance);
        let pod_new_source_commitment: pod::PedersenCommitment = new_source_commitment.into();

        transcript.append_commitment(b"commitment-new-source", &pod_new_source_commitment);

        // generate equality_proof
        let equality_proof = CiphertextCommitmentEqualityProof::new(
            source_keypair,
            new_source_ciphertext,
            &opening_source,
            source_new_balance,
            transcript,
        );

        // generate ciphertext validity proof
        let ciphertext_amount_validity_proof = BatchedGroupedCiphertext2HandlesValidityProof::new(
            (destination_pubkey, auditor_pubkey),
            (transfer_amount_lo, transfer_amount_hi),
            (opening_lo, opening_hi),
            transcript,
        );

        // compute claimed delta commitment
        let (claimed_commitment, opening_claimed) = Pedersen::new(delta_fee);
        let pod_claimed_commitment: pod::PedersenCommitment = claimed_commitment.into();
        transcript.append_commitment(b"commitment-claimed", &pod_claimed_commitment);

        let combined_commitment = combine_lo_hi_commitments(
            ciphertext_lo.get_commitment(),
            ciphertext_hi.get_commitment(),
            TRANSFER_AMOUNT_LO_BITS,
        );
        let combined_opening =
            combine_lo_hi_openings(opening_lo, opening_hi, TRANSFER_AMOUNT_LO_BITS);

        let combined_fee_amount =
            combine_lo_hi_u64(fee_amount_lo, fee_amount_hi, TRANSFER_AMOUNT_LO_BITS);
        let combined_fee_commitment = combine_lo_hi_commitments(
            fee_ciphertext_lo.get_commitment(),
            fee_ciphertext_hi.get_commitment(),
            TRANSFER_AMOUNT_LO_BITS,
        );
        let combined_fee_opening =
            combine_lo_hi_openings(opening_fee_lo, opening_fee_hi, TRANSFER_AMOUNT_LO_BITS);

        // compute real delta commitment
        let (delta_commitment, opening_delta) = compute_delta_commitment_and_opening(
            (&combined_commitment, &combined_opening),
            (&combined_fee_commitment, &combined_fee_opening),
            fee_parameters.fee_rate_basis_points,
        );
        let pod_delta_commitment: pod::PedersenCommitment = delta_commitment.into();
        transcript.append_commitment(b"commitment-delta", &pod_delta_commitment);

        // generate fee sigma proof
        let fee_sigma_proof = FeeSigmaProof::new(
            (
                combined_fee_amount,
                &combined_fee_commitment,
                &combined_fee_opening,
            ),
            (delta_fee, &delta_commitment, &opening_delta),
            (&claimed_commitment, &opening_claimed),
            fee_parameters.maximum_fee,
            transcript,
        );

        // generate ciphertext validity proof for fee ciphertexts
        let fee_ciphertext_validity_proof = BatchedGroupedCiphertext2HandlesValidityProof::new(
            (destination_pubkey, withdraw_withheld_authority_pubkey),
            (fee_amount_lo, fee_amount_hi),
            (opening_fee_lo, opening_fee_hi),
            transcript,
        );

        // generate the range proof
        let opening_claimed_negated = &PedersenOpening::default() - &opening_claimed;
        let range_proof = RangeProof::new(
            vec![
                source_new_balance,
                transfer_amount_lo,
                transfer_amount_hi,
                delta_fee,
                MAX_FEE_BASIS_POINTS - delta_fee,
                fee_amount_lo,
                fee_amount_hi,
            ],
            vec![
                TRANSFER_SOURCE_AMOUNT_BITS, // 64
                TRANSFER_AMOUNT_LO_BITS,     // 16
                TRANSFER_AMOUNT_HI_BITS,     // 32
                TRANSFER_DELTA_BITS,         // 48
                TRANSFER_DELTA_BITS,         // 48
                FEE_AMOUNT_LO_BITS,          // 16
                FEE_AMOUNT_HI_BITS,          // 32
            ],
            vec![
                &opening_source,
                opening_lo,
                opening_hi,
                &opening_claimed,
                &opening_claimed_negated,
                opening_fee_lo,
                opening_fee_hi,
            ],
            transcript,
        )
        .expect("range proof: generator");

        Self {
            new_source_commitment: pod_new_source_commitment,
            claimed_commitment: pod_claimed_commitment,
            equality_proof: equality_proof.into(),
            ciphertext_amount_validity_proof: ciphertext_amount_validity_proof.into(),
            fee_sigma_proof: fee_sigma_proof.into(),
            fee_ciphertext_validity_proof: fee_ciphertext_validity_proof.into(),
            range_proof: range_proof.try_into().expect("range proof: length error"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn verify(
        &self,
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        withdraw_withheld_authority_pubkey: &ElGamalPubkey,
        ciphertext_lo: &TransferAmountCiphertext,
        ciphertext_hi: &TransferAmountCiphertext,
        new_spendable_ciphertext: &ElGamalCiphertext,
        // fee parameters
        fee_ciphertext_lo: &FeeEncryption,
        fee_ciphertext_hi: &FeeEncryption,
        fee_parameters: FeeParameters,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        transcript.append_commitment(b"commitment-new-source", &self.new_source_commitment);

        let new_source_commitment: PedersenCommitment = self.new_source_commitment.try_into()?;
        let claimed_commitment: PedersenCommitment = self.claimed_commitment.try_into()?;

        let equality_proof: CiphertextCommitmentEqualityProof = self.equality_proof.try_into()?;
        let ciphertext_amount_validity_proof: BatchedGroupedCiphertext2HandlesValidityProof =
            self.ciphertext_amount_validity_proof.try_into()?;
        let fee_sigma_proof: FeeSigmaProof = self.fee_sigma_proof.try_into()?;
        let fee_ciphertext_validity_proof: BatchedGroupedCiphertext2HandlesValidityProof =
            self.fee_ciphertext_validity_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // verify equality proof
        equality_proof.verify(
            source_pubkey,
            new_spendable_ciphertext,
            &new_source_commitment,
            transcript,
        )?;

        // verify that the transfer amount is encrypted correctly
        ciphertext_amount_validity_proof.verify(
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

        // verify fee sigma proof
        transcript.append_commitment(b"commitment-claimed", &self.claimed_commitment);

        let combined_commitment = combine_lo_hi_commitments(
            ciphertext_lo.get_commitment(),
            ciphertext_hi.get_commitment(),
            TRANSFER_AMOUNT_LO_BITS,
        );
        let combined_fee_commitment = combine_lo_hi_commitments(
            fee_ciphertext_lo.get_commitment(),
            fee_ciphertext_hi.get_commitment(),
            TRANSFER_AMOUNT_LO_BITS,
        );

        let delta_commitment = compute_delta_commitment(
            &combined_commitment,
            &combined_fee_commitment,
            fee_parameters.fee_rate_basis_points,
        );

        let pod_delta_commitment: pod::PedersenCommitment = delta_commitment.into();
        transcript.append_commitment(b"commitment-delta", &pod_delta_commitment);

        // verify fee sigma proof
        fee_sigma_proof.verify(
            &combined_fee_commitment,
            &delta_commitment,
            &claimed_commitment,
            fee_parameters.maximum_fee,
            transcript,
        )?;

        // verify ciphertext validity proof for fee ciphertexts
        fee_ciphertext_validity_proof.verify(
            (destination_pubkey, withdraw_withheld_authority_pubkey),
            (
                fee_ciphertext_lo.get_commitment(),
                fee_ciphertext_hi.get_commitment(),
            ),
            (
                fee_ciphertext_lo.get_destination_handle(),
                fee_ciphertext_hi.get_destination_handle(),
            ),
            (
                fee_ciphertext_lo.get_withdraw_withheld_authority_handle(),
                fee_ciphertext_hi.get_withdraw_withheld_authority_handle(),
            ),
            transcript,
        )?;

        // verify range proof
        let new_source_commitment = self.new_source_commitment.try_into()?;
        let claimed_commitment_negated = &(*COMMITMENT_MAX_FEE_BASIS_POINTS) - &claimed_commitment;

        range_proof.verify(
            vec![
                &new_source_commitment,
                ciphertext_lo.get_commitment(),
                ciphertext_hi.get_commitment(),
                &claimed_commitment,
                &claimed_commitment_negated,
                fee_ciphertext_lo.get_commitment(),
                fee_ciphertext_hi.get_commitment(),
            ],
            vec![
                TRANSFER_SOURCE_AMOUNT_BITS, // 64
                TRANSFER_AMOUNT_LO_BITS,     // 16
                TRANSFER_AMOUNT_HI_BITS,     // 32
                TRANSFER_DELTA_BITS,         // 48
                TRANSFER_DELTA_BITS,         // 48
                FEE_AMOUNT_LO_BITS,          // 16
                FEE_AMOUNT_HI_BITS,          // 32
            ],
            transcript,
        )?;

        Ok(())
    }
}

#[cfg(not(target_os = "solana"))]
fn calculate_fee(transfer_amount: u64, fee_rate_basis_points: u16) -> Option<(u64, u64)> {
    let numerator = (transfer_amount as u128).checked_mul(fee_rate_basis_points as u128)?;

    // Warning: Division may involve CPU opcodes that have variable execution times. This
    // non-constant-time execution of the fee calculation can theoretically reveal information
    // about the transfer amount. For transfers that invole extremely sensitive data, additional
    // care should be put into how the fees are calculated.
    let fee = numerator
        .checked_add(ONE_IN_BASIS_POINTS)?
        .checked_sub(1)?
        .checked_div(ONE_IN_BASIS_POINTS)?;

    let delta_fee = fee
        .checked_mul(ONE_IN_BASIS_POINTS)?
        .checked_sub(numerator)?;

    Some((fee as u64, delta_fee as u64))
}

#[cfg(not(target_os = "solana"))]
fn compute_delta_commitment_and_opening(
    (combined_commitment, combined_opening): (&PedersenCommitment, &PedersenOpening),
    (combined_fee_commitment, combined_fee_opening): (&PedersenCommitment, &PedersenOpening),
    fee_rate_basis_points: u16,
) -> (PedersenCommitment, PedersenOpening) {
    let fee_rate_scalar = Scalar::from(fee_rate_basis_points);
    let delta_commitment = combined_fee_commitment * Scalar::from(MAX_FEE_BASIS_POINTS)
        - combined_commitment * &fee_rate_scalar;
    let delta_opening = combined_fee_opening * Scalar::from(MAX_FEE_BASIS_POINTS)
        - combined_opening * &fee_rate_scalar;

    (delta_commitment, delta_opening)
}

#[cfg(not(target_os = "solana"))]
fn compute_delta_commitment(
    combined_commitment: &PedersenCommitment,
    combined_fee_commitment: &PedersenCommitment,
    fee_rate_basis_points: u16,
) -> PedersenCommitment {
    let fee_rate_scalar = Scalar::from(fee_rate_basis_points);
    combined_fee_commitment * Scalar::from(MAX_FEE_BASIS_POINTS)
        - combined_commitment * &fee_rate_scalar
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_fee_correctness() {
        let source_keypair = ElGamalKeypair::new_rand();

        let destination_keypair = ElGamalKeypair::new_rand();
        let destination_pubkey = destination_keypair.pubkey();

        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pubkey = auditor_keypair.pubkey();

        let withdraw_withheld_authority_keypair = ElGamalKeypair::new_rand();
        let withdraw_withheld_authority_pubkey = withdraw_withheld_authority_keypair.pubkey();

        // Case 1: transfer 0 amount
        let spendable_balance: u64 = 120;
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        let transfer_amount: u64 = 0;

        let fee_parameters = FeeParameters {
            fee_rate_basis_points: 400,
            maximum_fee: 3,
        };

        let fee_data = TransferWithFeeData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (destination_pubkey, auditor_pubkey),
            fee_parameters,
            withdraw_withheld_authority_pubkey,
        )
        .unwrap();

        assert!(fee_data.verify_proof().is_ok());

        // Case 2: transfer max amount
        let spendable_balance: u64 = u64::max_value();
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        let transfer_amount: u64 =
            (1u64 << (TRANSFER_AMOUNT_LO_BITS + TRANSFER_AMOUNT_HI_BITS)) - 1;

        let fee_parameters = FeeParameters {
            fee_rate_basis_points: 400,
            maximum_fee: 3,
        };

        let fee_data = TransferWithFeeData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (destination_pubkey, auditor_pubkey),
            fee_parameters,
            withdraw_withheld_authority_pubkey,
        )
        .unwrap();

        assert!(fee_data.verify_proof().is_ok());

        // Case 3: general success case
        let spendable_balance: u64 = 120;
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        let transfer_amount: u64 = 100;

        let fee_parameters = FeeParameters {
            fee_rate_basis_points: 400,
            maximum_fee: 3,
        };

        let fee_data = TransferWithFeeData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (destination_pubkey, auditor_pubkey),
            fee_parameters,
            withdraw_withheld_authority_pubkey,
        )
        .unwrap();

        assert!(fee_data.verify_proof().is_ok());

        // Case 4: destination pubkey invalid
        let spendable_balance: u64 = 120;
        let spendable_ciphertext = source_keypair.pubkey().encrypt(spendable_balance);

        let transfer_amount: u64 = 0;

        let fee_parameters = FeeParameters {
            fee_rate_basis_points: 400,
            maximum_fee: 3,
        };

        // destination pubkey invalid
        let destination_pubkey: ElGamalPubkey = pod::ElGamalPubkey::zeroed().try_into().unwrap();

        let auditor_keypair = ElGamalKeypair::new_rand();
        let auditor_pubkey = auditor_keypair.pubkey();

        let withdraw_withheld_authority_keypair = ElGamalKeypair::new_rand();
        let withdraw_withheld_authority_pubkey = withdraw_withheld_authority_keypair.pubkey();

        let fee_data = TransferWithFeeData::new(
            transfer_amount,
            (spendable_balance, &spendable_ciphertext),
            &source_keypair,
            (&destination_pubkey, auditor_pubkey),
            fee_parameters,
            withdraw_withheld_authority_pubkey,
        )
        .unwrap();

        assert!(fee_data.verify_proof().is_err());
    }
}
