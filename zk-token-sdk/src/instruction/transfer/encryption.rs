#[cfg(not(target_os = "solana"))]
use crate::encryption::{
    elgamal::{DecryptHandle, ElGamalPubkey},
    grouped_elgamal::{GroupedElGamal, GroupedElGamalCiphertext},
    pedersen::{PedersenCommitment, PedersenOpening},
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
#[cfg(not(target_os = "solana"))]
pub struct FeeEncryption(pub(crate) GroupedElGamalCiphertext<2>);

#[cfg(not(target_os = "solana"))]
impl FeeEncryption {
    pub fn new(
        amount: u64,
        destination_pubkey: &ElGamalPubkey,
        withdraw_withheld_authority_pubkey: &ElGamalPubkey,
    ) -> (Self, PedersenOpening) {
        let opening = PedersenOpening::new_rand();
        let grouped_ciphertext = GroupedElGamal::<2>::encrypt_with(
            [destination_pubkey, withdraw_withheld_authority_pubkey],
            amount,
            &opening,
        );

        (Self(grouped_ciphertext), opening)
    }

    pub fn get_commitment(&self) -> &PedersenCommitment {
        &self.0.commitment
    }

    pub fn get_destination_handle(&self) -> &DecryptHandle {
        // `FeeEncryption` is a wrapper for `GroupedElGamalCiphertext<2>`, which holds
        // exactly two decryption handles.
        self.0.handles.get(0).unwrap()
    }

    pub fn get_withdraw_withheld_authority_handle(&self) -> &DecryptHandle {
        // `FeeEncryption` is a wrapper for `GroupedElGamalCiphertext<2>`, which holds
        // exactly two decryption handles.
        self.0.handles.get(1).unwrap()
    }
}
