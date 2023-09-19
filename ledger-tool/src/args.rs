use {
    crate::LEDGER_TOOL_DIRECTORY,
    clap::{value_t, values_t_or_exit, ArgMatches},
    solana_accounts_db::{
        accounts_db::{AccountsDb, AccountsDbConfig, FillerAccountsConfig},
        accounts_index::{AccountsIndexConfig, IndexLimitMb},
        partitioned_rewards::TestPartitionedEpochRewards,
    },
    solana_runtime::snapshot_utils,
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
    let ledger_tool_ledger_path = ledger_path.join(LEDGER_TOOL_DIRECTORY);
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
        vec![ledger_tool_ledger_path.join("accounts_index")]
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

    let accounts_hash_cache_path = arg_matches
        .value_of("accounts_hash_cache_path")
        .map(Into::into)
        .unwrap_or_else(|| {
            ledger_tool_ledger_path.join(AccountsDb::DEFAULT_ACCOUNTS_HASH_CACHE_DIR)
        });
    let accounts_hash_cache_path =
        snapshot_utils::create_and_canonicalize_directories(&[accounts_hash_cache_path])
            .unwrap_or_else(|err| {
                eprintln!("Unable to access accounts hash cache path: {err}");
                std::process::exit(1);
            })
            .pop()
            .unwrap();

    AccountsDbConfig {
        index: Some(accounts_index_config),
        base_working_path: Some(ledger_tool_ledger_path),
        accounts_hash_cache_path: Some(accounts_hash_cache_path),
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
