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
            combine_u32_ciphertexts, combine_u32_commitments, combine_u32_openings,
            split_u64_into_u32,
            transfer::{TransferAmountEncryption, TransferPubkeys},
            Role, Verifiable,
        },
        range_proof::RangeProof,
        sigma_proofs::{
            equality_proof::EqualityProof,
            fee_proof::FeeSigmaProof,
            validity_proof::{AggregatedValidityProof, ValidityProof},
        },
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
    pub transfer_with_fee_pubkeys: pod::TransferWithFeePubkeys,

    /// The final spendable ciphertext after the transfer,
    pub ciphertext_new_source: pod::ElGamalCiphertext,

    // transfer fee encryption
    pub ciphertext_fee: pod::FeeEncryption,

    // fee parameters
    pub fee_parameters: pod::FeeParameters,

    // transfer fee proof
    pub proof: TransferWithFeeProof,
}

#[cfg(not(target_arch = "bpf"))]
impl TransferWithFeeData {
    pub fn new(
        transfer_amount: u64,
        (spendable_balance, ciphertext_old_source): (u64, &ElGamalCiphertext),
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

        let ciphertext_new_source = ciphertext_old_source
            - combine_u32_ciphertexts(&transfer_amount_lo_source, &transfer_amount_hi_source);

        // calculate and encrypt fee
        let (fee_amount, delta_fee) =
            calculate_fee(transfer_amount, fee_parameters.fee_rate_basis_points);

        let above_max = u64::ct_gt(&fee_parameters.maximum_fee, &fee_amount);
        let fee_to_encrypt =
            u64::conditional_select(&fee_amount, &fee_parameters.maximum_fee, above_max);

        let (ciphertext_fee, opening_fee) =
            FeeEncryption::new(fee_to_encrypt, pubkey_dest, pubkey_fee_collector);

        // generate transcript and append all public inputs
        let pod_transfer_with_fee_pubkeys = pod::TransferWithFeePubkeys::new(
            &keypair_source.public,
            pubkey_dest,
            pubkey_auditor,
            pubkey_fee_collector,
        );
        let pod_ciphertext_lo: pod::TransferAmountEncryption = ciphertext_lo.into();
        let pod_ciphertext_hi: pod::TransferAmountEncryption = ciphertext_hi.into();
        let pod_ciphertext_new_source: pod::ElGamalCiphertext = ciphertext_new_source.into();
        let pod_ciphertext_fee: pod::FeeEncryption = ciphertext_fee.into();

        let mut transcript = TransferWithFeeProof::transcript_new(
            &pod_transfer_with_fee_pubkeys,
            &pod_ciphertext_lo,
            &pod_ciphertext_hi,
            &pod_ciphertext_fee,
        );

        let proof = TransferWithFeeProof::new(
            (amount_lo, &ciphertext_lo, &opening_lo),
            (amount_hi, &ciphertext_hi, &opening_hi),
            keypair_source,
            (pubkey_dest, pubkey_auditor),
            (new_spendable_balance, &ciphertext_new_source),
            (fee_amount, &ciphertext_fee, &opening_fee),
            delta_fee,
            pubkey_fee_collector,
            fee_parameters,
            &mut transcript,
        );

        Self {
            ciphertext_lo: pod_ciphertext_lo,
            ciphertext_hi: pod_ciphertext_hi,
            transfer_with_fee_pubkeys: pod_transfer_with_fee_pubkeys,
            ciphertext_new_source: pod_ciphertext_new_source,
            ciphertext_fee: pod_ciphertext_fee,
            fee_parameters: fee_parameters.into(),
            proof,
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
pub struct TransferWithFeeProof {
    pub commitment_new_source: pod::PedersenCommitment,
    pub equality_proof: pod::EqualityProof,
    pub ciphertext_amount_validity_proof: pod::AggregatedValidityProof,
    pub fee_sigma_proof: pod::FeeSigmaProof,
    pub ciphertext_fee_validity_proof: pod::ValidityProof,
    pub range_proof: pod::RangeProof256,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl TransferWithFeeProof {
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
        transcript.append_message(b"ciphertext-fee", &ciphertext_fee.0);

        transcript
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::many_single_char_names)]
    pub fn new(
        transfer_amount_lo_data: (u32, &TransferAmountEncryption, &PedersenOpening),
        transfer_amount_hi_data: (u32, &TransferAmountEncryption, &PedersenOpening),
        keypair_source: &ElGamalKeypair,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        (source_new_balance, ciphertext_new_source): (u64, &ElGamalCiphertext),

        (fee_amount, ciphertext_fee, opening_fee): (u64, &FeeEncryption, &PedersenOpening),
        delta_fee: u64,
        pubkey_fee_collector: &ElGamalPubkey,
        fee_parameters: FeeParameters,
        transcript: &mut Transcript,
    ) -> Self {
        let (transfer_amount_lo, ciphertext_lo, opening_lo) = transfer_amount_lo_data;
        let (transfer_amount_hi, ciphertext_hi, opening_hi) = transfer_amount_hi_data;

        // generate a Pedersen commitment for the remaining balance in source
        let (commitment_new_source, opening_source) = Pedersen::new(source_new_balance);

        // generate equality_proof
        let equality_proof = EqualityProof::new(
            keypair_source,
            ciphertext_new_source,
            source_new_balance,
            &opening_source,
            transcript,
        );

        // generate ciphertext validity proof
        let ciphertext_amount_validity_proof = AggregatedValidityProof::new(
            (pubkey_dest, pubkey_auditor),
            (transfer_amount_lo, transfer_amount_hi),
            (opening_lo, opening_hi),
            transcript,
        );

        let (commitment_delta, opening_delta) = compute_delta_commitment(
            (&ciphertext_lo.commitment, opening_lo),
            (&ciphertext_hi.commitment, opening_hi),
            (&ciphertext_fee.commitment, opening_fee),
            fee_parameters.fee_rate_basis_points,
        );

        let (commitment_claimed, opening_claimed) = Pedersen::new(delta_fee);

        let fee_sigma_proof = FeeSigmaProof::new(
            (fee_amount, &ciphertext_fee.commitment, opening_fee),
            (delta_fee, &commitment_delta, &opening_delta),
            (&commitment_claimed, &opening_claimed),
            fee_parameters.maximum_fee,
            transcript,
        );

        let ciphertext_fee_validityProof = ValidityProof::new(
            (pubkey_dest, pubkey_auditor),
            fee_amount,
            opening_fee,
            transcript,
        );

        let opening_claimed_negated = &PedersenOpening::default() - &opening_claimed;
        let range_proof = RangeProof::new(
            vec![
                source_new_balance,
                transfer_amount_lo as u64,
                transfer_amount_hi as u64,
                delta_fee,
                10000_u64 - delta_fee,
            ],
            vec![
                64, 32, 32, 64, // double check
                64,
            ],
            vec![
                &opening_source,
                opening_lo,
                opening_hi,
                &opening_claimed,
                &opening_claimed_negated,
            ],
            transcript,
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

impl pod::TransferWithFeePubkeys {
    pub fn new(
        source: &ElGamalPubkey,
        dest: &ElGamalPubkey,
        auditor: &ElGamalPubkey,
        fee_collector: &ElGamalPubkey,
    ) -> Self {
        let mut bytes = [0u8; 128];
        bytes[..32].copy_from_slice(&source.to_bytes());
        bytes[32..64].copy_from_slice(&dest.to_bytes());
        bytes[64..96].copy_from_slice(&auditor.to_bytes());
        bytes[64..128].copy_from_slice(&fee_collector.to_bytes());
        Self(bytes)
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

fn compute_delta_commitment(
    (commitment_lo, opening_lo): (&PedersenCommitment, &PedersenOpening),
    (commitment_hi, opening_hi): (&PedersenCommitment, &PedersenOpening),
    (commitment_fee, opening_fee): (&PedersenCommitment, &PedersenOpening),
    fee_rate_basis_points: u16,
) -> (PedersenCommitment, PedersenOpening) {
    let fee_rate_scalar = Scalar::from(fee_rate_basis_points);

    let commitment_delta = commitment_fee * Scalar::from(10000_u64)
        - &combine_u32_commitments(&commitment_lo, &commitment_hi);

    let opening_delta =
        opening_fee * Scalar::from(10000_u64) - &combine_u32_openings(&opening_lo, &opening_hi);

    (commitment_delta, opening_delta)
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

        let dest_pubkey = ElGamalKeypair::new_rand().public;
        let fee_collector_pubkey = ElGamalKeypair::new_rand().public;

        let fee_data = TransferWithFeeData::new(
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
