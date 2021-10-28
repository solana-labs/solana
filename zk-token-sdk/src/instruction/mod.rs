mod close_account;
mod transfer;
mod withdraw;

#[cfg(not(target_arch = "bpf"))]
use crate::errors::ProofError;
pub use {
    close_account::CloseAccountData,
    transfer::{TransferCommitments, TransferData, TransferPubKeys},
    withdraw::WithdrawData,
};

#[cfg(not(target_arch = "bpf"))]
pub trait Verifiable {
    fn verify(&self) -> Result<(), ProofError>;
}

#[cfg(not(target_arch = "bpf"))]
#[derive(Debug, Copy, Clone)]
pub enum Role {
    Source,
    Dest,
    Auditor,
}
