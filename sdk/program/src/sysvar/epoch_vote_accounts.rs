//! this account contains the sorted list of vote accounts and their active stake, if non-zero, for
//! the current epoch
//!
pub use crate::epoch_vote_accounts::EpochVoteAccounts;

use crate::sysvar::Sysvar;

crate::declare_sysvar_id!(
    "SysvarEpochVoteAccounts11111111111111111111",
    EpochVoteAccounts
);

impl Sysvar for EpochVoteAccounts {}
