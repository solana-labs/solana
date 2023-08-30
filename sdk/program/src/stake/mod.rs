//! The [stake native program][np].
//!
//! [np]: https://docs.solana.com/developing/runtime-facilities/sysvars#stakehistory

#[allow(deprecated)]
pub mod config;
pub mod instruction;
pub mod stake_flags;
pub mod state;
pub mod tools;

mod deprecated;
pub use deprecated::*;

pub mod program {
    crate::declare_id!("Stake11111111111111111111111111111111111111");
}

/// The minimum number of epochs before stake account that is delegated to a delinquent vote
/// account may be unstaked with `StakeInstruction::DeactivateDelinquent`
pub const MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION: usize = 5;
