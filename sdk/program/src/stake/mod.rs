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

/// The minimum number of epochs before stake account that is delegated to a delinquent vote
/// account may be unstaked with `StakeInstruction::DeactivateDelinquent`
pub const MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION: usize = 5;
