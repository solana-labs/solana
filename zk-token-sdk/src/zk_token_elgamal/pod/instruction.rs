use crate::zk_token_elgamal::pod::{
    elgamal::{ElGamalCiphertext, DECRYPT_HANDLE_LEN, ELGAMAL_CIPHERTEXT_LEN},
    pedersen::{PedersenCommitment, PEDERSEN_COMMITMENT_LEN},
    GroupedElGamalCiphertext2Handles, GroupedElGamalCiphertext3Handles, Pod, PodU16, PodU64,
    Zeroable,
};
#[cfg(not(target_os = "solana"))]
use crate::{errors::ProofError, instruction::transfer as decoded};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferAmountCiphertext(pub GroupedElGamalCiphertext3Handles);

#[cfg(not(target_os = "solana"))]
impl From<decoded::TransferAmountCiphertext> for TransferAmountCiphertext {
    fn from(decoded_ciphertext: decoded::TransferAmountCiphertext) -> Self {
        Self(decoded_ciphertext.0.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<TransferAmountCiphertext> for decoded::TransferAmountCiphertext {
    type Error = ProofError;

    fn try_from(pod_ciphertext: TransferAmountCiphertext) -> Result<Self, Self::Error> {
        Ok(Self(pod_ciphertext.0.try_into()?))
    }
}

// A `TransferAmountCiphertext` contains the following sequence of elements in order:
//   - Pedersen commitment of the transfer amount (32 bytes)
//   - Decrypt handle with respect to the source ElGamal public key (32 bytes)
//   - Decrypt handle with respect to the destination ElGamal public key (32 bytes)
//   - Decrypt handle with respect to the auditor ElGamal public key (32 bytes)
impl TransferAmountCiphertext {
    /// Extract the first 32 bytes of `TransferAmountCiphertext` as `PedersenCommitment`.
    pub fn get_commitment(&self) -> PedersenCommitment {
        let transfer_amount_ciphertext_bytes = bytemuck::bytes_of(self);
        let mut commitment_bytes = [0u8; PEDERSEN_COMMITMENT_LEN];
        commitment_bytes
            .copy_from_slice(&transfer_amount_ciphertext_bytes[..PEDERSEN_COMMITMENT_LEN]);
        PedersenCommitment(commitment_bytes)
    }

    /// Extract the first 64 bytes of `TransferAmountCiphertext` as `ElGamalCiphertext` pertaining
    /// to the source ElGamal public key.
    pub fn get_source_ciphertext(&self) -> ElGamalCiphertext {
        let transfer_amount_ciphertext_bytes = bytemuck::bytes_of(self);
        let mut source_ciphertext_bytes = [0u8; ELGAMAL_CIPHERTEXT_LEN];
        source_ciphertext_bytes
            .copy_from_slice(&transfer_amount_ciphertext_bytes[..ELGAMAL_CIPHERTEXT_LEN]);
        ElGamalCiphertext(source_ciphertext_bytes)
    }

    /// Extract the first and third 32 bytes of `TransferAmountCiphertext` as `ElGamalCiphertext`
    /// pertaining to the destination ElGamal public key.
    pub fn get_destination_ciphertext(&self) -> ElGamalCiphertext {
        let transfer_amount_ciphertext_bytes = bytemuck::bytes_of(self);
        let mut destination_ciphertext_bytes = [0u8; ELGAMAL_CIPHERTEXT_LEN];
        destination_ciphertext_bytes[..PEDERSEN_COMMITMENT_LEN]
            .copy_from_slice(&transfer_amount_ciphertext_bytes[..PEDERSEN_COMMITMENT_LEN]);
        destination_ciphertext_bytes[PEDERSEN_COMMITMENT_LEN..].copy_from_slice(
            &transfer_amount_ciphertext_bytes
                [ELGAMAL_CIPHERTEXT_LEN..ELGAMAL_CIPHERTEXT_LEN + DECRYPT_HANDLE_LEN],
        );
        ElGamalCiphertext(destination_ciphertext_bytes)
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeEncryption(pub GroupedElGamalCiphertext2Handles);

#[cfg(not(target_os = "solana"))]
impl From<decoded::FeeEncryption> for FeeEncryption {
    fn from(decoded_ciphertext: decoded::FeeEncryption) -> Self {
        Self(decoded_ciphertext.0.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<FeeEncryption> for decoded::FeeEncryption {
    type Error = ProofError;

    fn try_from(pod_ciphertext: FeeEncryption) -> Result<Self, Self::Error> {
        Ok(Self(pod_ciphertext.0.try_into()?))
    }
}

// A `FeeEncryption` contains the following sequence of elements in order:
//   - Pedersen commitment of the transfer amount (32 bytes)
//   - Decrypt handle with respect to the destination ElGamal public key (32 bytes)
//   - Decrypt handle with respect to the withdraw withheld authority ElGamal public key (32 bytes)
impl FeeEncryption {
    /// Extract the first 32 bytes of `FeeEncryption` as `PedersenCommitment`.
    pub fn get_commitment(&self) -> PedersenCommitment {
        let fee_encryption_bytes = bytemuck::bytes_of(self);
        let mut commitment_bytes = [0u8; PEDERSEN_COMMITMENT_LEN];
        commitment_bytes.copy_from_slice(&fee_encryption_bytes[..PEDERSEN_COMMITMENT_LEN]);
        PedersenCommitment(commitment_bytes)
    }

    /// Extract the first 64 bytes of `FeeEncryption` as `ElGamalCiphertext` pertaining
    /// to the destination ElGamal public key.
    pub fn get_destination_ciphertext(&self) -> ElGamalCiphertext {
        let fee_encryption_bytes = bytemuck::bytes_of(self);
        let mut destination_ciphertext_bytes = [0u8; ELGAMAL_CIPHERTEXT_LEN];
        destination_ciphertext_bytes
            .copy_from_slice(&fee_encryption_bytes[..ELGAMAL_CIPHERTEXT_LEN]);
        ElGamalCiphertext(destination_ciphertext_bytes)
    }

    /// Extract the first and third 32 bytes of `FeeEncryption` as `ElGamalCiphertext`
    /// pertaining to the withdraw withheld authority ElGamal public key.
    pub fn get_withdraw_withheld_authority_ciphertext(&self) -> ElGamalCiphertext {
        let fee_encryption_bytes = bytemuck::bytes_of(self);
        let mut withdraw_withheld_authority_ciphertext_bytes = [0u8; ELGAMAL_CIPHERTEXT_LEN];
        withdraw_withheld_authority_ciphertext_bytes[..PEDERSEN_COMMITMENT_LEN]
            .copy_from_slice(&fee_encryption_bytes[..PEDERSEN_COMMITMENT_LEN]);
        withdraw_withheld_authority_ciphertext_bytes[PEDERSEN_COMMITMENT_LEN..].copy_from_slice(
            &fee_encryption_bytes
                [ELGAMAL_CIPHERTEXT_LEN..ELGAMAL_CIPHERTEXT_LEN + DECRYPT_HANDLE_LEN],
        );
        ElGamalCiphertext(withdraw_withheld_authority_ciphertext_bytes)
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: PodU16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: PodU64,
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::FeeParameters> for FeeParameters {
    fn from(decoded_fee_parameters: decoded::FeeParameters) -> Self {
        FeeParameters {
            fee_rate_basis_points: decoded_fee_parameters.fee_rate_basis_points.into(),
            maximum_fee: decoded_fee_parameters.maximum_fee.into(),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl From<FeeParameters> for decoded::FeeParameters {
    fn from(pod_fee_parameters: FeeParameters) -> Self {
        decoded::FeeParameters {
            fee_rate_basis_points: pod_fee_parameters.fee_rate_basis_points.into(),
            maximum_fee: pod_fee_parameters.maximum_fee.into(),
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{
            elgamal::ElGamalKeypair, grouped_elgamal::GroupedElGamal, pedersen::Pedersen,
        },
    };

    #[test]
    fn test_transfer_amount_ciphertext_byte_conversions() {
        let source_keypair = ElGamalKeypair::new_rand();
        let destination_keypair = ElGamalKeypair::new_rand();
        let auditor_keypair = ElGamalKeypair::new_rand();

        let source_pubkey = source_keypair.pubkey();
        let destination_pubkey = destination_keypair.pubkey();
        let auditor_pubkey = auditor_keypair.pubkey();

        let transfer_amount = 5_u64;
        let (commitment, opening) = Pedersen::new(transfer_amount);

        let grouped_elgamal_ciphertext = GroupedElGamal::encrypt_with(
            [source_pubkey, destination_pubkey, auditor_pubkey],
            transfer_amount,
            &opening,
        );

        let pod_grouped_elgamal_ciphertext: GroupedElGamalCiphertext3Handles =
            grouped_elgamal_ciphertext.into();
        let pod_transfer_amount_ciphertext =
            TransferAmountCiphertext(pod_grouped_elgamal_ciphertext);

        let pod_commitment: PedersenCommitment = commitment.into();
        assert_eq!(
            pod_commitment,
            pod_transfer_amount_ciphertext.get_commitment()
        );

        let source_decrypt_handle = source_pubkey.decrypt_handle(&opening);
        let mut source_ciphertext_bytes = [0; ELGAMAL_CIPHERTEXT_LEN];
        source_ciphertext_bytes[..32].copy_from_slice(&commitment.to_bytes());
        source_ciphertext_bytes[32..].copy_from_slice(&source_decrypt_handle.to_bytes());
        let pod_source_ciphertext = ElGamalCiphertext(source_ciphertext_bytes);

        assert_eq!(
            pod_source_ciphertext,
            pod_transfer_amount_ciphertext.get_source_ciphertext(),
        );

        let destination_decrypt_handle = destination_pubkey.decrypt_handle(&opening);
        let mut destination_ciphertext_bytes = [0; ELGAMAL_CIPHERTEXT_LEN];
        destination_ciphertext_bytes[..32].copy_from_slice(&commitment.to_bytes());
        destination_ciphertext_bytes[32..].copy_from_slice(&destination_decrypt_handle.to_bytes());
        let pod_destination_ciphertext = ElGamalCiphertext(destination_ciphertext_bytes);

        assert_eq!(
            pod_destination_ciphertext,
            pod_transfer_amount_ciphertext.get_destination_ciphertext(),
        );
    }

    #[test]
    fn test_fee_encryption_byte_conversions() {
        let destination_keypair = ElGamalKeypair::new_rand();
        let withdraw_withheld_authority_keypair = ElGamalKeypair::new_rand();

        let destination_pubkey = destination_keypair.pubkey();
        let withdraw_withheld_authority_pubkey = withdraw_withheld_authority_keypair.pubkey();

        let fee_amount = 5_u64;
        let (commitment, opening) = Pedersen::new(fee_amount);

        let grouped_elgamal_ciphertext = GroupedElGamal::encrypt_with(
            [destination_pubkey, withdraw_withheld_authority_pubkey],
            fee_amount,
            &opening,
        );

        let pod_grouped_elgamal_ciphertext: GroupedElGamalCiphertext2Handles =
            grouped_elgamal_ciphertext.into();
        let pod_fee_encryption = FeeEncryption(pod_grouped_elgamal_ciphertext);

        let pod_commitment: PedersenCommitment = commitment.into();
        assert_eq!(pod_commitment, pod_fee_encryption.get_commitment());

        let destination_decrypt_handle = destination_pubkey.decrypt_handle(&opening);
        let mut destination_ciphertext_bytes = [0; ELGAMAL_CIPHERTEXT_LEN];
        destination_ciphertext_bytes[..32].copy_from_slice(&commitment.to_bytes());
        destination_ciphertext_bytes[32..].copy_from_slice(&destination_decrypt_handle.to_bytes());
        let pod_destination_ciphertext = ElGamalCiphertext(destination_ciphertext_bytes);

        assert_eq!(
            pod_destination_ciphertext,
            pod_fee_encryption.get_destination_ciphertext(),
        );

        let withdraw_withheld_authority_decrypt_handle =
            withdraw_withheld_authority_pubkey.decrypt_handle(&opening);
        let mut withdraw_withheld_authority_ciphertext_bytes = [0; ELGAMAL_CIPHERTEXT_LEN];
        withdraw_withheld_authority_ciphertext_bytes[..32].copy_from_slice(&commitment.to_bytes());
        withdraw_withheld_authority_ciphertext_bytes[32..]
            .copy_from_slice(&withdraw_withheld_authority_decrypt_handle.to_bytes());
        let pod_withdraw_withheld_authority_ciphertext =
            ElGamalCiphertext(withdraw_withheld_authority_ciphertext_bytes);

        assert_eq!(
            pod_withdraw_withheld_authority_ciphertext,
            pod_fee_encryption.get_withdraw_withheld_authority_ciphertext(),
        );
    }
}
