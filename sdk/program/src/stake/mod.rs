pub mod config;
pub mod instruction;
pub mod state;

pub mod program {
    crate::declare_id!("Stake11111111111111111111111111111111111111");
}

#[deprecated(
    since = "1.10.6",
    note = "This constant may be outdated, please use `solana_stake_program::get_minimum_delegation` instead"
)]
pub const MINIMUM_STAKE_DELEGATION: u64 = 1;
