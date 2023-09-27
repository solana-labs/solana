#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::arithmetic_side_effects)]
#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::program::id` or `solana_program::stake::program::id` instead"
)]
pub use solana_sdk::stake::program::{check_id, id};
use solana_sdk::{
    feature_set::{self, FeatureSet},
    genesis_config::GenesisConfig,
    native_token::LAMPORTS_PER_SOL,
};

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
pub fn get_minimum_delegation(feature_set: &FeatureSet) -> u64 {
    if feature_set.is_active(&feature_set::stake_raise_minimum_delegation_to_1_sol::id()) {
        const MINIMUM_DELEGATION_SOL: u64 = 1;
        MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
    } else {
        #[allow(deprecated)]
        solana_sdk::stake::MINIMUM_STAKE_DELEGATION
    }
}
