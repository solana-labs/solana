pub mod config;
pub mod instruction;
pub mod state;
pub mod tools;

pub mod program {
    crate::declare_id!("Stake11111111111111111111111111111111111111");
}

// NOTE: This constant will be deprecated soon; if possible, use
// `solana_stake_program::get_minimum_delegation()` instead.
pub const MINIMUM_STAKE_DELEGATION: u64 = 1;
