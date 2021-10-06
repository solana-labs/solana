mod close_account;
mod transfer;
mod update_account_pk;
mod withdraw;

#[cfg(not(target_arch = "bpf"))]
use crate::errors::ProofError;
pub use {
    close_account::CloseAccountData,
    transfer::{TransferCommitments, TransferData, TransferPubKeys},
    update_account_pk::UpdateAccountPkData,
    withdraw::WithdrawData,
};

#[cfg(not(target_arch = "bpf"))]
pub trait Verifiable {
    fn verify(&self) -> Result<(), ProofError>;
}
