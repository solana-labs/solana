
//! # Examples
//!
//! ```
//! # use solana_program::{
//! #    account_info::{AccountInfo, next_account_info},
//! #    entrypoint::ProgramResult,
//! #    msg,
//! #    pubkey::Pubkey,
//! #    sysvar::Sysvar,
//! # };
//! #
//! # use solana_program::last_restart_slot::LastRestartSlot;
//! # use solana_program::program_error::ProgramError;
//! # use solana_program::sysvar::last_restart_slot;
//! #
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!     let last_restart_slot_account_info = next_account_info(account_info_iter)?;
//!
//!     assert!(last_restart_slot::check_id(last_restart_slot_account_info.key));
//!
//!     let slot = LastRestartSlot::from_account_info(last_restart_slot_account_info)?;
//!     msg!("last restart slot: {:#?}", slot);
//!
//!     Ok(())
//! }
//! ```
use solana_sdk_macro::CloneZeroed;

pub type Slot = u64;

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, CloneZeroed, PartialEq, Eq)]
pub struct LastRestartSlot {
    /// The last restart `Slot`.
    pub last_restart_slot: Slot,
}

impl Default for LastRestartSlot {
    fn default() -> Self {
        Self {
            last_restart_slot: 0,
        }
    }
}

// TODO
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_clone() {
//         let clock = Clock {
//             slot: 1,
//             epoch_start_timestamp: 2,
//             epoch: 3,
//             leader_schedule_epoch: 4,
//             unix_timestamp: 5,
//         };
//         let cloned_clock = clock.clone();
//         assert_eq!(cloned_clock, clock);
//     }
// }
