use {
    crate::bank::Bank,
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
    pub(crate) vote_accounts_cache_miss_count: usize,
    pub(crate) hash_partition_rewards_us: u64,
}

pub(crate) struct NewBankTimings {
    pub(crate) bank_rc_creation_time_us: u64,
    pub(crate) total_elapsed_time_us: u64,
    pub(crate) status_cache_time_us: u64,
    pub(crate) fee_components_time_us: u64,
    pub(crate) blockhash_queue_time_us: u64,
    pub(crate) stakes_cache_time_us: u64,
    pub(crate) epoch_stakes_time_us: u64,
    pub(crate) builtin_programs_time_us: u64,
    pub(crate) rewards_pool_pubkeys_time_us: u64,
    pub(crate) executor_cache_time_us: u64,
    pub(crate) transaction_debug_keys_time_us: u64,
    pub(crate) transaction_log_collector_config_time_us: u64,
    pub(crate) feature_set_time_us: u64,
    pub(crate) ancestors_time_us: u64,
    pub(crate) update_epoch_time_us: u64,
    pub(crate) recompilation_time_us: u64,
    pub(crate) update_sysvars_time_us: u64,
    pub(crate) fill_sysvar_cache_time_us: u64,
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
            "vote_accounts_cache_miss_count",
            metrics.vote_accounts_cache_miss_count,
            i64
        ),
        (
            "hash_partition_rewards_us",
            metrics.hash_partition_rewards_us,
            i64
        ),
    );
}

pub(crate) fn report_new_bank_metrics(
    slot: Slot,
    parent_slot: Slot,
    block_height: u64,
    timings: NewBankTimings,
) {
    datapoint_info!(
        "bank-new_from_parent-heights",
        ("slot", slot, i64),
        ("block_height", block_height, i64),
        ("parent_slot", parent_slot, i64),
        ("bank_rc_creation_us", timings.bank_rc_creation_time_us, i64),
        ("total_elapsed_us", timings.total_elapsed_time_us, i64),
        ("status_cache_us", timings.status_cache_time_us, i64),
        ("fee_components_us", timings.fee_components_time_us, i64),
        ("blockhash_queue_us", timings.blockhash_queue_time_us, i64),
        ("stakes_cache_us", timings.stakes_cache_time_us, i64),
        ("epoch_stakes_time_us", timings.epoch_stakes_time_us, i64),
        ("builtin_programs_us", timings.builtin_programs_time_us, i64),
        (
            "rewards_pool_pubkeys_us",
            timings.rewards_pool_pubkeys_time_us,
            i64
        ),
        ("executor_cache_us", timings.executor_cache_time_us, i64),
        (
            "transaction_debug_keys_us",
            timings.transaction_debug_keys_time_us,
            i64
        ),
        (
            "transaction_log_collector_config_us",
            timings.transaction_log_collector_config_time_us,
            i64
        ),
        ("feature_set_us", timings.feature_set_time_us, i64),
        ("ancestors_us", timings.ancestors_time_us, i64),
        ("update_epoch_us", timings.update_epoch_time_us, i64),
        ("recompilation_time_us", timings.recompilation_time_us, i64),
        ("update_sysvars_us", timings.update_sysvars_time_us, i64),
        (
            "fill_sysvar_cache_us",
            timings.fill_sysvar_cache_time_us,
            i64
        ),
    );
}

/// Metrics for partitioned epoch reward store
#[derive(Debug, Default)]
pub(crate) struct RewardsStoreMetrics {
    pub(crate) partition_index: u64,
    pub(crate) store_stake_accounts_us: u64,
    pub(crate) store_stake_accounts_count: usize,
    pub(crate) total_stake_accounts_count: usize,
    pub(crate) distributed_rewards: u64,
    pub(crate) pre_capitalization: u64,
    pub(crate) post_capitalization: u64,
}

#[allow(dead_code)]
pub(crate) fn report_partitioned_reward_metrics(bank: &Bank, timings: RewardsStoreMetrics) {
    datapoint_info!(
        "bank-partitioned_epoch_rewards_credit",
        ("slot", bank.slot(), i64),
        ("epoch", bank.epoch(), i64),
        ("block_height", bank.block_height(), i64),
        ("parent_slot", bank.parent_slot(), i64),
        ("partition_index", timings.partition_index, i64),
        (
            "store_stake_accounts_us",
            timings.store_stake_accounts_us,
            i64
        ),
        (
            "store_stake_accounts_count",
            timings.store_stake_accounts_count,
            i64
        ),
        (
            "total_stake_accounts_count",
            timings.total_stake_accounts_count,
            i64
        ),
        ("distributed_rewards", timings.distributed_rewards, i64),
        ("pre_capitalization", timings.pre_capitalization, i64),
        ("post_capitalization", timings.post_capitalization, i64),
    );
}
