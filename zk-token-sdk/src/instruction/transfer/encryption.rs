#[cfg(not(target_os = "solana"))]
use crate::{
    encryption::{
        elgamal::{DecryptHandle, ElGamalPubkey},
        pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
    },
    zk_token_elgamal::pod,
};

// TransferAmountEncryption
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

// FeeEncryption
#[derive(Clone)]
#[repr(C)]
#[cfg(not(target_os = "solana"))]
pub struct FeeEncryption {
    pub commitment: PedersenCommitment,
    pub destination_handle: DecryptHandle,
    pub withdraw_withheld_authority_handle: DecryptHandle,
}

#[cfg(not(target_os = "solana"))]
impl FeeEncryption {
    pub fn new(
        amount: u64,
        destination_pubkey: &ElGamalPubkey,
        withdraw_withheld_authority_pubkey: &ElGamalPubkey,
    ) -> (Self, PedersenOpening) {
        let (commitment, opening) = Pedersen::new(amount);
        let fee_encryption = Self {
            commitment,
            destination_handle: destination_pubkey.decrypt_handle(&opening),
            withdraw_withheld_authority_handle: withdraw_withheld_authority_pubkey
                .decrypt_handle(&opening),
        };

        (fee_encryption, opening)
    }

    pub fn to_pod(&self) -> pod::FeeEncryption {
        pod::FeeEncryption {
            commitment: self.commitment.into(),
            destination_handle: self.destination_handle.into(),
            withdraw_withheld_authority_handle: self.withdraw_withheld_authority_handle.into(),
        }
    }
}
