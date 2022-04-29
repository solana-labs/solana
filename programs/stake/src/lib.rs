#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]
#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::program::id` or `solana_program::stake::program::id` instead"
)]
pub use solana_sdk::stake::program::{check_id, id};
use solana_sdk::{feature_set::FeatureSet, genesis_config::GenesisConfig};

pub mod config;
pub mod stake_instruction;
pub mod stake_state;

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig) -> u64 {
    config::add_genesis_account(genesis_config)
}

/// The minimum stake amount that can be delegated, in lamports.
/// NOTE: This is also used to calculate the minimum balance of a stake account, which is the
/// rent exempt reserve _plus_ the minimum stake delegation.
#[inline(always)]
pub fn get_minimum_delegation(_feature_set: &FeatureSet) -> u64 {
    // If/when the minimum delegation amount is changed, the `feature_set` parameter will be used
    // to chose the correct value.  And since the MINIMUM_STAKE_DELEGATION constant cannot be
    // removed, use it here as to not duplicate magic constants.
    #[allow(deprecated)]
    solana_sdk::stake::MINIMUM_STAKE_DELEGATION
}
