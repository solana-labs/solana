mod close_account;
pub mod transfer;
mod update_account_pk;
mod withdraw;

#[cfg(not(target_arch = "bpf"))]
use crate::errors::ProofError;
pub use {
    close_account::CloseAccountData,
    transfer::{
        TransferComms, TransferData, TransferEphemeralState, TransferPubKeys,
        TransferRangeProofData, TransferValidityProofData,
    },
    update_account_pk::UpdateAccountPkData,
    withdraw::WithdrawData,
};

#[cfg(not(target_arch = "bpf"))]
pub trait Verifiable {
    fn verify(&self) -> Result<(), ProofError>;
}
