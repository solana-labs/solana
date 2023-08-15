use {
    clap::{value_t, values_t_or_exit, ArgMatches},
    solana_runtime::{
        accounts_db::{AccountsDb, AccountsDbConfig, FillerAccountsConfig},
        accounts_index::{AccountsIndexConfig, IndexLimitMb},
        partitioned_rewards::TestPartitionedEpochRewards,
    },
    solana_sdk::clock::Slot,
    std::path::{Path, PathBuf},
};

// Build an `AccountsDbConfig` from subcommand arguments. All of the arguments
// matched by this functional are either optional or have a default value.
// Thus, a subcommand need not support all of the arguments that are matched
// by this function.
pub fn get_accounts_db_config(
    ledger_path: &Path,
    arg_matches: &ArgMatches<'_>,
) -> AccountsDbConfig {
    let accounts_index_bins = value_t!(arg_matches, "accounts_index_bins", usize).ok();
    let accounts_index_index_limit_mb =
        if let Ok(limit) = value_t!(arg_matches, "accounts_index_memory_limit_mb", usize) {
            IndexLimitMb::Limit(limit)
        } else if arg_matches.is_present("disable_accounts_disk_index") {
            IndexLimitMb::InMemOnly
        } else {
            IndexLimitMb::Unspecified
        };
    let test_partitioned_epoch_rewards =
        if arg_matches.is_present("partitioned_epoch_rewards_compare_calculation") {
            TestPartitionedEpochRewards::CompareResults
        } else if arg_matches.is_present("partitioned_epoch_rewards_force_enable_single_slot") {
            TestPartitionedEpochRewards::ForcePartitionedEpochRewardsInOneBlock
        } else {
            TestPartitionedEpochRewards::None
        };

    let accounts_index_drives: Vec<PathBuf> = if arg_matches.is_present("accounts_index_path") {
        values_t_or_exit!(arg_matches, "accounts_index_path", String)
            .into_iter()
            .map(PathBuf::from)
            .collect()
    } else {
        vec![ledger_path.join("accounts_index.ledger-tool")]
    };
    let accounts_index_config = AccountsIndexConfig {
        bins: accounts_index_bins,
        index_limit_mb: accounts_index_index_limit_mb,
        drives: Some(accounts_index_drives),
        ..AccountsIndexConfig::default()
    };

    let filler_accounts_config = FillerAccountsConfig {
        count: value_t!(arg_matches, "accounts_filler_count", usize).unwrap_or(0),
        size: value_t!(arg_matches, "accounts_filler_size", usize).unwrap_or(0),
    };

    AccountsDbConfig {
        index: Some(accounts_index_config),
        accounts_hash_cache_path: Some(
            ledger_path.join(AccountsDb::DEFAULT_ACCOUNTS_HASH_CACHE_DIR),
        ),
        base_working_path: Some(ledger_path.to_path_buf()),
        filler_accounts_config,
        ancient_append_vec_offset: value_t!(arg_matches, "accounts_db_ancient_append_vecs", i64)
            .ok(),
        exhaustively_verify_refcounts: arg_matches.is_present("accounts_db_verify_refcounts"),
        skip_initial_hash_calc: arg_matches.is_present("accounts_db_skip_initial_hash_calculation"),
        test_partitioned_epoch_rewards,
        ..AccountsDbConfig::default()
    }
}

// This function is duplicated in validator/src/main.rs...
pub fn hardforks_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<Slot>> {
    if matches.is_present(name) {
        Some(values_t_or_exit!(matches, name, Slot))
    } else {
        None
    }
}
