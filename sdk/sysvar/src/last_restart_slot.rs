//! Information about the last restart slot (hard fork).
//!
//! The _last restart sysvar_ provides access to the last restart slot kept in the
//! bank fork for the slot on the fork that executes the current transaction.
//! In case there was no fork it returns _0_.
//!
//! [`LastRestartSlot`] implements [`Sysvar::get`] and can be loaded efficiently without
//! passing the sysvar account ID to the program.
//!
//! See also the Solana [SIMD proposal][simd].
//!
//! [simd]: https://github.com/solana-foundation/solana-improvement-documents/blob/main/proposals/0047-syscall-and-sysvar-for-last-restart-slot.md
//!
//! # Examples
//!
//! Accessing via on-chain program directly:
//!
//! ```no_run
//! # use solana_account_info::AccountInfo;
//! # use solana_msg::msg;
//! # use solana_sysvar::Sysvar;
//! # use solana_program_error::ProgramResult;
//! # use solana_pubkey::Pubkey;
//! # use solana_last_restart_slot::LastRestartSlot;
//!
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!
//!     let last_restart_slot = LastRestartSlot::get();
//!     msg!("last restart slot: {:?}", last_restart_slot);
//!
//!     Ok(())
//! }
//! ```
//!
#[cfg(feature = "bincode")]
use crate::{impl_sysvar_get, Sysvar};
pub use {
    solana_last_restart_slot::LastRestartSlot,
    solana_sdk_ids::sysvar::last_restart_slot::{check_id, id, ID},
};

#[cfg(feature = "bincode")]
impl Sysvar for LastRestartSlot {
    impl_sysvar_get!(sol_get_last_restart_slot);
}
