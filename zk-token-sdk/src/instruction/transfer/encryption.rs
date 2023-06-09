#[cfg(not(target_os = "solana"))]
use crate::{
    encryption::{
        elgamal::{DecryptHandle, ElGamalPubkey},
        grouped_elgamal::{GroupedElGamal, GroupedElGamalCiphertext},
        pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
    },
    zk_token_elgamal::pod,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
#[cfg(not(target_os = "solana"))]
pub struct TransferAmountCiphertext(pub(crate) GroupedElGamalCiphertext<3>);

#[cfg(not(target_os = "solana"))]
impl TransferAmountCiphertext {
    pub fn new(
        amount: u64,
        source_pubkey: &ElGamalPubkey,
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
    ) -> (Self, PedersenOpening) {
        let opening = PedersenOpening::new_rand();
        let grouped_ciphertext = GroupedElGamal::<3>::encrypt_with(
            [source_pubkey, destination_pubkey, auditor_pubkey],
            amount,
            &opening,
        );

        (Self(grouped_ciphertext), opening)
    }

    pub fn get_commitment(&self) -> &PedersenCommitment {
        &self.0.commitment
    }

    pub fn get_source_handle(&self) -> &DecryptHandle {
        // `TransferAmountCiphertext` is a wrapper for `GroupedElGamalCiphertext<3>`, which
        // holds exactly three decryption handles.
        self.0.handles.get(0).unwrap()
    }

    pub fn get_destination_handle(&self) -> &DecryptHandle {
        // `TransferAmountCiphertext` is a wrapper for `GroupedElGamalCiphertext<3>`, which holds
        // exactly three decryption handles.
        self.0.handles.get(1).unwrap()
    }

    pub fn get_auditor_handle(&self) -> &DecryptHandle {
        // `TransferAmountCiphertext` is a wrapper for `GroupedElGamalCiphertext<3>`, which holds
        // exactly three decryption handles.
        self.0.handles.get(2).unwrap()
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
