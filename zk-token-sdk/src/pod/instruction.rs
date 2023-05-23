use crate::{
    errors::ProofError,
    instruction::{transfer, transfer_with_fee},
    pod::{elgamal::*, pedersen::*, Pod, PodU16, PodU64, Zeroable},
};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferPubkeys {
    pub source_pubkey: ElGamalPubkey,
    pub destination_pubkey: ElGamalPubkey,
    pub auditor_pubkey: ElGamalPubkey,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferWithFeePubkeys {
    pub source_pubkey: ElGamalPubkey,
    pub destination_pubkey: ElGamalPubkey,
    pub auditor_pubkey: ElGamalPubkey,
    pub withdraw_withheld_authority_pubkey: ElGamalPubkey,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferAmountEncryption {
    pub commitment: PedersenCommitment,
    pub source_handle: DecryptHandle,
    pub destination_handle: DecryptHandle,
    pub auditor_handle: DecryptHandle,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeEncryption {
    pub commitment: PedersenCommitment,
    pub destination_handle: DecryptHandle,
    pub withdraw_withheld_authority_handle: DecryptHandle,
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
impl From<transfer::TransferPubkeys> for TransferPubkeys {
    fn from(keys: transfer::TransferPubkeys) -> Self {
        Self {
            source_pubkey: keys.source_pubkey.into(),
            destination_pubkey: keys.destination_pubkey.into(),
            auditor_pubkey: keys.auditor_pubkey.into(),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<TransferPubkeys> for transfer::TransferPubkeys {
    type Error = ProofError;

    fn try_from(pod: TransferPubkeys) -> Result<Self, Self::Error> {
        Ok(Self {
            source_pubkey: pod.source_pubkey.try_into()?,
            destination_pubkey: pod.destination_pubkey.try_into()?,
            auditor_pubkey: pod.auditor_pubkey.try_into()?,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl From<transfer_with_fee::TransferWithFeePubkeys> for TransferWithFeePubkeys {
    fn from(keys: transfer_with_fee::TransferWithFeePubkeys) -> Self {
        Self {
            source_pubkey: keys.source_pubkey.into(),
            destination_pubkey: keys.destination_pubkey.into(),
            auditor_pubkey: keys.auditor_pubkey.into(),
            withdraw_withheld_authority_pubkey: keys.withdraw_withheld_authority_pubkey.into(),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<TransferWithFeePubkeys> for transfer_with_fee::TransferWithFeePubkeys {
    type Error = ProofError;

    fn try_from(pod: TransferWithFeePubkeys) -> Result<Self, Self::Error> {
        Ok(Self {
            source_pubkey: pod.source_pubkey.try_into()?,
            destination_pubkey: pod.destination_pubkey.try_into()?,
            auditor_pubkey: pod.auditor_pubkey.try_into()?,
            withdraw_withheld_authority_pubkey: pod
                .withdraw_withheld_authority_pubkey
                .try_into()?,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl From<transfer::TransferAmountEncryption> for TransferAmountEncryption {
    fn from(ciphertext: transfer::TransferAmountEncryption) -> Self {
        Self {
            commitment: ciphertext.commitment.into(),
            source_handle: ciphertext.source_handle.into(),
            destination_handle: ciphertext.destination_handle.into(),
            auditor_handle: ciphertext.auditor_handle.into(),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<TransferAmountEncryption> for transfer::TransferAmountEncryption {
    type Error = ProofError;

    fn try_from(pod: TransferAmountEncryption) -> Result<Self, Self::Error> {
        Ok(Self {
            commitment: pod.commitment.try_into()?,
            source_handle: pod.source_handle.try_into()?,
            destination_handle: pod.destination_handle.try_into()?,
            auditor_handle: pod.auditor_handle.try_into()?,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl From<transfer_with_fee::FeeEncryption> for FeeEncryption {
    fn from(ciphertext: transfer_with_fee::FeeEncryption) -> Self {
        Self {
            commitment: ciphertext.commitment.into(),
            destination_handle: ciphertext.destination_handle.into(),
            withdraw_withheld_authority_handle: ciphertext
                .withdraw_withheld_authority_handle
                .into(),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<FeeEncryption> for transfer_with_fee::FeeEncryption {
    type Error = ProofError;

    fn try_from(pod: FeeEncryption) -> Result<Self, Self::Error> {
        Ok(Self {
            commitment: pod.commitment.try_into()?,
            destination_handle: pod.destination_handle.try_into()?,
            withdraw_withheld_authority_handle: pod
                .withdraw_withheld_authority_handle
                .try_into()?,
        })
    }
}

#[cfg(not(target_os = "solana"))]
impl From<transfer_with_fee::FeeParameters> for FeeParameters {
    fn from(parameters: transfer_with_fee::FeeParameters) -> Self {
        Self {
            fee_rate_basis_points: parameters.fee_rate_basis_points.into(),
            maximum_fee: parameters.maximum_fee.into(),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl From<FeeParameters> for transfer_with_fee::FeeParameters {
    fn from(pod: FeeParameters) -> Self {
        Self {
            fee_rate_basis_points: pod.fee_rate_basis_points.into(),
            maximum_fee: pod.maximum_fee.into(),
        }
    }
}
