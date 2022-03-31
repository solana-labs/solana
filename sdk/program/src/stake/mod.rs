//! The [stake native program][np].
//!
//! [np]: https://docs.solana.com/developing/runtime-facilities/sysvars#stakehistory

pub mod config;
pub mod instruction;
pub mod state;

pub mod program {
    crate::declare_id!("Stake11111111111111111111111111111111111111");
}

// NOTE: This constant will be deprecated soon; if possible, use
// `solana_stake_program::get_minimum_delegation()` instead.
pub const MINIMUM_STAKE_DELEGATION: u64 = 1;
