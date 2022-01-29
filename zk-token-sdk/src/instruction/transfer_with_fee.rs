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
        instruction::{
            combine_u32_ciphertexts, combine_u32_commitments, split_u64_into_u32,
            transfer::{TransferAmountEncryption, TransferPubkeys},
            Role, Verifiable,
        },
        range_proof::RangeProof,
        sigma_proofs::{fee_proof::FeeSigmaProof, validity_proof::ValidityProof},
        transcript::TranscriptProtocol,
    },
    arrayref::{array_ref, array_refs},
    curve25519_dalek::scalar::Scalar,
    merlin::Transcript,
    std::convert::TryInto,
    subtle::{Choice, ConditionallySelectable, ConstantTimeGreater},
};

// #[derive(Clone, Copy, Pod, Zeroable)]
#[derive(Clone)]
#[repr(C)]
pub struct TransferWithFeeData {
    /// Group encryption of the low 32 bites of the transfer amount
    pub ciphertext_lo: pod::TransferAmountEncryption,

    /// Group encryption of the high 32 bits of the transfer amount
    pub ciphertext_hi: pod::TransferAmountEncryption,

    /// The public encryption keys associated with the transfer: source, dest, and auditor
    pub transfer_pubkeys: pod::TransferWithFeePubkeys,

    /// The final spendable ciphertext after the transfer,
    pub new_spendable_ciphertext: pod::ElGamalCiphertext,

    // transfer fee encryption
    pub fee_ciphertext: pod::FeeEncryption,

    // fee parameters
    pub fee_parameters: pod::FeeParameters,

    // transfer fee proof
    pub transfer_fee_proof: TransferWithFeeProof,
}

#[cfg(not(target_arch = "bpf"))]
impl TransferWithFeeData {
    pub fn new(
        transfer_amount: u64,
        (spendable_balance, spendable_balance_ciphertext): (u64, &ElGamalCiphertext),
        keypair_source: &ElGamalKeypair,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        fee_parameters: FeeParameters,
        pubkey_fee_collector: &ElGamalPubkey,
    ) -> Self {
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
        let new_spendable_balance = spendable_balance - transfer_amount;

        let transfer_amount_lo_source = ElGamalCiphertext {
            commitment: ciphertext_lo.commitment,
            handle: ciphertext_lo.source,
        };

        let transfer_amount_hi_source = ElGamalCiphertext {
            commitment: ciphertext_hi.commitment,
            handle: ciphertext_hi.source,
        };

        let new_spendable_ciphertext = spendable_balance_ciphertext
            - combine_u32_ciphertexts(&transfer_amount_lo_source, &transfer_amount_hi_source);

        // calculate and encrypt fee
        let (fee_amount, delta_fee) =
            calculate_fee(transfer_amount, fee_parameters.fee_rate_basis_points);

        let above_max = u64::ct_gt(&fee_parameters.maximum_fee, &fee_amount);
        let fee_to_encrypt =
            u64::conditional_select(&fee_amount, &fee_parameters.maximum_fee, above_max);

        let (ciphertext_fee, opening_fee) =
            FeeEncryption::new(fee_to_encrypt, pubkey_dest, pubkey_fee_collector);

        let proof = TransferWithFeeProof::new(
            (
                transfer_amount,
                &transfer_amount_commitment,
                &transfer_amount_opening,
            ),
            (
                fee_to_commit,
                &transfer_fee_commitment,
                &transfer_fee_opening,
            ),
            delta_fee,
            (&pubkey_dest, &pubkey_fee_collector),
            fee_parameters,
        );

        // TODO: pod for fee parameters
        Self {
            transfer_amount_commitment: transfer_amount_commitment.into(),
            transfer_fee_commitment: transfer_fee_commitment.into(),
            decrypt_handle_dest: decrypt_handle_dest.into(),
            decrypt_handle_fee_collector: decrypt_handle_fee_collector.into(),
            pubkey_dest: pubkey_dest.into(),
            pubkey_fee_collector: pubkey_fee_collector.into(),
            fee_parameters,
            transfer_fee_proof,
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for FeeData {
    fn verify(&self) -> Result<(), ProofError> {
        let transfer_amount_commitment: PedersenCommitment =
            self.transfer_amount_commitment.try_into()?;
        let transfer_fee_commitment: PedersenCommitment =
            self.transfer_fee_commitment.try_into()?;
        let decrypt_handle_dest: PedersenDecryptHandle = self.decrypt_handle_dest.try_into()?;
        let decrypt_handle_fee_collector: PedersenDecryptHandle =
            self.decrypt_handle_fee_collector.try_into()?;
        let pubkey_dest = self.pubkey_dest.try_into()?;
        let pubkey_fee_collector = self.pubkey_fee_collector.try_into()?;
        let proof = self.transfer_fee_proof.clone();

        proof.verify(
            &transfer_amount_commitment,
            &transfer_fee_commitment,
            &decrypt_handle_dest,
            &decrypt_handle_fee_collector,
            &pubkey_dest,
            &pubkey_fee_collector,
            self.fee_parameters,
        )
    }
}

// #[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[derive(Clone)]
pub struct FeeProof {
    pub delta_claimed_commitment: pod::PedersenCommitment,
    pub fee_sigma_proof: pod::FeeSigmaProof,
    pub fee_validity_proof: pod::ValidityProof,
    pub fee_range_proof: RangeProof, // will ultimately be aggregated, so use non-pod for now
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl FeeProof {
    fn transcript_new(
        transfer_with_fee_pubkeys: &pod::TransferWithFeePubkeys,
        ciphertext_lo: &pod::TransferAmountEncryption,
        ciphertext_hi: &pod::TransferAmountEncryption,
        ciphertext_fee: &pod::FeeEncryption,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"FeeProof");

        transcript.append_message(b"transfer-with-fee-pubkeys", &transfer_with_fee_pubkeys.0);
        transcript.append_message(b"ciphertext-lo", &ciphertext_lo.0);
        transcript.append_message(b"ciphertext-hi", &ciphertext_hi.0);
        transcript.append_message(b"ciphertext-fee", ciphertext_fee.0);

        transcript
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::many_single_char_names)]
    pub fn new(
        transfer_amount_lo_data: (u32, &TransferAmountEncryption, &PedersenOpening),
        transfer_amount_hi_data: (u32, &TransferAmountEncryption, &PedersenOpening),
        keypair_source: &ElGamalKeypair,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        (source_new_balance, source_new_balance_ciphertext): (u64, &ElGamalCiphertext),

        (fee_amount, ciphertext_fee, opening_fee): (u64, &FeeEncryption, &PedersenOpening),
        delta_fee: u64,
        pubkey_fee_collector: &ElGamalPubkey,

        // transfer_amount_data: (u64, &PedersenCommitment, &PedersenOpening),
        // fee_data: (u64, &PedersenCommitment, &PedersenOpening),
        // delta_fee: u64,
        fee_parameters: FeeParameters,
        transcript: &mut Transcript,
    ) -> Self {
        let (transfer_amount_lo, ciphertext_lo, opening_lo) = transfer_amount_lo_data;
        let (transfer_amount_hi, ciphertext_hi, opening_hi) = transfer_amount_hi_data;

        // generate a Pedersen commitment for the remaining balance in source
        let (source_commitment, source_opening) = Pedersen::new(source_new_balance);

        //------------------------------------------
        let fee_rate_scalar = Scalar::from(fee_parameters.fee_rate_basis_points);
        let commitment_delta = ciphertext_fee.commitment * Scalar::from(10000_u64)
            - &combine_u32_commitments(&ciphertext_lo.commitment, &ciphertext_hi.commitment);

        let opening_delta = opening_fee * Scalar::from(10000_u64)
            - &combine_u32_openings(&opening_lo, &opening_hi);

        // TODO: use a utility function
        // -------------------------------------------

        let (commitment_claimed, opening_claimed) = Pedersen::new(delta_fee);

        // TODO: sigma proofs for regular transfer

        // TODO: fee sigma proof

        // TODO: validity proof
        // TODO: range proof for both transfer and fee


        // 2. generate sigma proof
        //
        // TODO: consider grouping (commitment, value, opening) as a struct
        let fee_sigma_proof = FeeSigmaProof::new(
            fee_data,
            (delta_fee, &delta_fee_comm, &delta_fee_opening),
            (&delta_claimed_comm, &delta_claimed_opening),
            fee_parameters.maximum_fee,
            &mut transcript,
        );

        // 3. validity proof
        let validity_proof = ValidityProof::new(
            fee_data.0,
            (pubkey_dest, pubkey_fee_collector),
            fee_data.2,
            &mut transcript,
        );

        // 4. generate range proof
        //
        // TODO: double check copy
        // TODO: should ultimately be aggregated into other rangeproofs
        // TODO: validity proof
        let zero_opening = PedersenOpening::default();
        let delta_claimed_opening_range_proof = delta_claimed_opening.clone();
        let delta_claimed_negated_opening_range_proof =
            zero_opening - delta_claimed_opening.clone();
        let range_proof = RangeProof::new(
            vec![delta_fee, 10000_u64 - delta_fee],
            vec![16_usize, 16_usize],
            vec![
                &delta_claimed_opening_range_proof,
                &delta_claimed_negated_opening_range_proof,
            ],
            &mut transcript,
        );

        Self {
            delta_claimed_commitment: delta_claimed_comm.into(),
            fee_sigma_proof: fee_sigma_proof.into(),
            fee_validity_proof: validity_proof.into(),
            fee_range_proof: range_proof,
        }
    }

    pub fn verify(
        self,
        transfer_amount_commitment: &PedersenCommitment,
        fee_commitment: &PedersenCommitment,
        decrypt_handle_dest: &PedersenDecryptHandle,
        decrypt_handle_fee_collector: &PedersenDecryptHandle,
        pubkey_dest: &ElGamalPubkey,
        pubkey_fee_collector: &ElGamalPubkey,
        fee_parameters: FeeParameters,
    ) -> Result<(), ProofError> {
        let mut transcript = FeeProof::transcript_new();

        // compute delta commitments
        let fee_rate_scalar = Scalar::from(fee_parameters.fee_rate_basis_points);
        let delta_commitment =
            transfer_amount_commitment * fee_rate_scalar - fee_commitment * Scalar::from(10000_u64);
        let delta_claimed_commitment = self.delta_claimed_commitment.try_into()?;

        let fee_sigma_proof: FeeSigmaProof = self.fee_sigma_proof.try_into()?;
        fee_sigma_proof.verify(
            fee_parameters.maximum_fee,
            &fee_commitment,
            &delta_commitment,
            &delta_claimed_commitment,
            &mut transcript,
        )?;

        let validity_proof: ValidityProof = self.fee_validity_proof.try_into()?;
        validity_proof.verify(
            fee_commitment,
            (pubkey_dest, pubkey_fee_collector),
            (decrypt_handle_dest, decrypt_handle_fee_collector),
            &mut transcript,
        )?;

        let comm_10000 = Pedersen::with(10000_u64, &PedersenOpening::default());
        let delta_claimed_negated_opening_range_proof = comm_10000 - delta_claimed_commitment;

        let range_proof: RangeProof = self.fee_range_proof.clone();
        range_proof.verify(
            vec![
                &delta_claimed_commitment,
                &delta_claimed_negated_opening_range_proof,
            ],
            vec![16, 16],
            &mut transcript,
        )?;

        Ok(())
    }
}

/// The ElGamal public keys needed for a transfer with fee
#[derive(Clone)]
#[repr(C)]
pub struct TransferWithFeePubkeys {
    pub source: ElGamalPubkey,
    pub dest: ElGamalPubkey,
    pub auditor: ElGamalPubkey,
    pub fee_collector: ElGamalPubkey,
}

impl TransferWithFeePubkeys {
    pub fn to_bytes(&self) -> [u8; 128] {
        let mut bytes = [0u8; 128];
        bytes[..32].copy_from_slice(&self.source.to_bytes());
        bytes[32..64].copy_from_slice(&self.dest.to_bytes());
        bytes[64..96].copy_from_slice(&self.auditor.to_bytes());
        bytes[96..128].copy_from_slice(&self.fee_collector.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProofError> {
        let bytes = array_ref![bytes, 0, 128];
        let (source, dest, auditor, fee_collector) = array_refs![bytes, 32, 32, 32, 32];

        let source = ElGamalPubkey::from_bytes(source).ok_or(ProofError::Verification)?;
        let dest = ElGamalPubkey::from_bytes(dest).ok_or(ProofError::Verification)?;
        let auditor = ElGamalPubkey::from_bytes(auditor).ok_or(ProofError::Verification)?;
        let fee_collector =
            ElGamalPubkey::from_bytes(fee_collector).ok_or(ProofError::Verification)?;

        Ok(Self {
            source,
            dest,
            auditor,
            fee_collector,
        })
    }
}

#[derive(Clone)]
#[repr(C)]
pub struct FeeEncryption {
    pub commitment: PedersenCommitment,
    pub dest: DecryptHandle,
    pub fee_collector: DecryptHandle,
}

impl FeeEncryption {
    pub fn new(
        amount: u64,
        pubkey_dest: &ElGamalPubkey,
        pubkey_fee_collector: &ElGamalPubkey,
    ) -> (Self, PedersenOpening) {
        let (commitment, opening) = Pedersen::new(amount);
        let fee_encryption = Self {
            commitment,
            dest: pubkey_dest.decrypt_handle(&opening),
            fee_collector: pubkey_fee_collector.decrypt_handle(&opening),
        };

        (fee_encryption, opening)
    }

    pub fn to_bytes(&self) -> [u8; 96] {
        let mut bytes = [0u8; 96];
        bytes[..32].copy_from_slice(&self.commitment.to_bytes());
        bytes[32..64].copy_from_slice(&self.dest.to_bytes());
        bytes[64..96].copy_from_slice(&self.fee_collector.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProofError> {
        let bytes = array_ref![bytes, 0, 96];
        let (commitment, dest, fee_collector) = array_refs![bytes, 32, 32, 32];

        let commitment =
            PedersenCommitment::from_bytes(commitment).ok_or(ProofError::Verification)?;
        let dest = DecryptHandle::from_bytes(dest).ok_or(ProofError::Verification)?;
        let fee_collector =
            DecryptHandle::from_bytes(fee_collector).ok_or(ProofError::Verification)?;

        Ok(Self {
            commitment,
            dest,
            fee_collector,
        })
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: u16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: u64,
}

fn calculate_fee(transfer_amount: u64, fee_rate_basis_points: u16) -> (u64, u64) {
    let fee_scaled = (transfer_amount as u128) * (fee_rate_basis_points as u128);

    let fee = (fee_scaled / 10000) as u64;
    let rem = (fee_scaled % 10000) as u64;

    if rem == 0 {
        (fee, rem)
    } else {
        (fee + 1, rem)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_fee_correctness() {
        let transfer_amount: u64 = 100;
        let (transfer_amount_comm, transfer_amount_opening) = Pedersen::new(transfer_amount);

        let fee_parameters = FeeParameters {
            fee_rate_basis_points: 100,
            maximum_fee: 3,
        };

        let dest_pubkey = ElGamalKeypair::default().public;
        let fee_collector_pubkey = ElGamalKeypair::default().public;

        let fee_data = FeeData::new(
            transfer_amount,
            transfer_amount_comm,
            transfer_amount_opening,
            fee_parameters,
            dest_pubkey,
            fee_collector_pubkey,
        );

        assert!(fee_data.verify().is_ok());
    }
}
