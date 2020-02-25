//! This account contains the hash of the full accounts state at the accounts epoch
//!

pub use crate::accounts_epoch_hash::AccountsEpochHash;
use crate::sysvar::Sysvar;

crate::declare_sysvar_id!(
    "AccountsEpochHash11111111111111111111111111",
    AccountsEpochHash
);

impl Sysvar for AccountsEpochHash {}
