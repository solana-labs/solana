use {
    solana_sdk::clock::{Epoch, Slot},
    std::sync::atomic::{AtomicU64, Ordering::Relaxed},
};

pub(crate) struct NewEpochTimings {
    pub(crate) thread_pool_time_us: u64,
    pub(crate) apply_feature_activations_time_us: u64,
    pub(crate) activate_epoch_time_us: u64,
    pub(crate) update_epoch_stakes_time_us: u64,
    pub(crate) update_rewards_with_thread_pool_time_us: u64,
}

#[derive(Debug, Default)]
pub(crate) struct RewardsMetrics {
    pub(crate) load_vote_and_stake_accounts_us: AtomicU64,
    pub(crate) calculate_points_us: AtomicU64,
    pub(crate) redeem_rewards_us: u64,
    pub(crate) store_stake_accounts_us: AtomicU64,
    pub(crate) store_vote_accounts_us: AtomicU64,
    pub(crate) invalid_cached_vote_accounts: usize,
    pub(crate) invalid_cached_stake_accounts: usize,
    pub(crate) invalid_cached_stake_accounts_rent_epoch: usize,
    pub(crate) vote_accounts_cache_miss_count: usize,
}

pub(crate) fn report_new_epoch_metrics(
    epoch: Epoch,
    slot: Slot,
    parent_slot: Slot,
    timings: NewEpochTimings,
    metrics: RewardsMetrics,
) {
    datapoint_info!(
        "bank-new_from_parent-new_epoch_timings",
        ("epoch", epoch, i64),
        ("slot", slot, i64),
        ("parent_slot", parent_slot, i64),
        ("thread_pool_creation_us", timings.thread_pool_time_us, i64),
        (
            "apply_feature_activations",
            timings.apply_feature_activations_time_us,
            i64
        ),
        ("activate_epoch_us", timings.activate_epoch_time_us, i64),
        (
            "update_epoch_stakes_us",
            timings.update_epoch_stakes_time_us,
            i64
        ),
        (
            "update_rewards_with_thread_pool_us",
            timings.update_rewards_with_thread_pool_time_us,
            i64
        ),
        (
            "load_vote_and_stake_accounts_us",
            metrics.load_vote_and_stake_accounts_us.load(Relaxed),
            i64
        ),
        (
            "calculate_points_us",
            metrics.calculate_points_us.load(Relaxed),
            i64
        ),
        ("redeem_rewards_us", metrics.redeem_rewards_us, i64),
        (
            "store_stake_accounts_us",
            metrics.store_stake_accounts_us.load(Relaxed),
            i64
        ),
        (
            "store_vote_accounts_us",
            metrics.store_vote_accounts_us.load(Relaxed),
            i64
        ),
        (
            "invalid_cached_vote_accounts",
            metrics.invalid_cached_vote_accounts,
            i64
        ),
        (
            "invalid_cached_stake_accounts",
            metrics.invalid_cached_stake_accounts,
            i64
        ),
        (
            "invalid_cached_stake_accounts_rent_epoch",
            metrics.invalid_cached_stake_accounts_rent_epoch,
            i64
        ),
        (
            "vote_accounts_cache_miss_count",
            metrics.vote_accounts_cache_miss_count,
            i64
        ),
    );
}
