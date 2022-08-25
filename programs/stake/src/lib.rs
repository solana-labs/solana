#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
#![allow(clippy::integer_arithmetic)]
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
/// Multiple proposals to raise the delegation exist, enforce the feature with the highest minimum
#[inline(always)]
pub fn get_minimum_delegation(feature_set: &FeatureSet) -> u64 {
    if (feature_set.is_active(&feature_set::stake_raise_minimum_delegation_to_1_sol::id())
        && !feature_set
            .is_active(&feature_set::stake_raise_minimum_delegation_to_100m_lamports::id()))
        || ((feature_set.is_active(&feature_set::stake_raise_minimum_delegation_to_1_sol::id())
            && feature_set
                .is_active(&feature_set::stake_raise_minimum_delegation_to_100m_lamports::id()))
            && feature_set
                .activated_slot(&feature_set::stake_raise_minimum_delegation_to_1_sol::id())
                > feature_set.activated_slot(
                    &feature_set::stake_raise_minimum_delegation_to_100m_lamports::id(),
                ))
    {
        const MINIMUM_DELEGATION_SOL: u64 = 1;
        MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
    } else if feature_set
        .is_active(&feature_set::stake_raise_minimum_delegation_to_100m_lamports::id())
    {
        const MINIMUM_DELEGATION_LAMPORTS: u64 = 100_000_000;
        MINIMUM_DELEGATION_LAMPORTS
    } else {
        #[allow(deprecated)]
        solana_sdk::stake::MINIMUM_STAKE_DELEGATION
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_sdk::feature_set::FeatureSet, test_case::test_case};

    /// The "new" behavior enables all features
    fn feature_set_new_behavior() -> FeatureSet {
        FeatureSet::all_enabled()
    }

    fn feature_set_only_100m_lamports_minimum() -> FeatureSet {
        let mut feature_set = feature_set_new_behavior();
        feature_set.deactivate(&feature_set::stake_raise_minimum_delegation_to_1_sol::id());
        feature_set
    }

    fn feature_set_only_1_sol_minimum() -> FeatureSet {
        let mut feature_set = feature_set_new_behavior();
        feature_set.deactivate(&feature_set::stake_raise_minimum_delegation_to_100m_lamports::id());
        feature_set
    }

    /// The "old" behavior is before the stake minimum delegation was raised
    fn feature_set_old_behavior() -> FeatureSet {
        let mut feature_set = feature_set_new_behavior();
        feature_set.deactivate(&feature_set::stake_raise_minimum_delegation_to_1_sol::id());
        feature_set.deactivate(&feature_set::stake_raise_minimum_delegation_to_100m_lamports::id());
        feature_set
    }

    #[allow(deprecated)]
    #[test_case(feature_set_old_behavior(), solana_sdk::stake::MINIMUM_STAKE_DELEGATION; "old_behavior")]
    #[test_case(feature_set_only_1_sol_minimum(), 1 * LAMPORTS_PER_SOL; "new_behavior_1_sol")]
    #[test_case(feature_set_only_100m_lamports_minimum(), 100_000_000; "new_behavior_100m_lamports")]
    fn test_minimum_delegation(feature_set: FeatureSet, expected_result: u64) {
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        assert_eq!(minimum_delegation, expected_result)
    }
}
