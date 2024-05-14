use {
    crate::LEDGER_TOOL_DIRECTORY,
    clap::{value_t, value_t_or_exit, values_t, values_t_or_exit, ArgMatches},
    solana_accounts_db::{
        accounts_db::{AccountsDb, AccountsDbConfig},
        accounts_index::{AccountsIndexConfig, IndexLimitMb},
        partitioned_rewards::TestPartitionedEpochRewards,
        utils::create_and_canonicalize_directories,
    },
    solana_clap_utils::input_parsers::pubkeys_of,
    solana_ledger::{
        blockstore_processor::ProcessOptions,
        use_snapshot_archives_at_startup::{self, UseSnapshotArchivesAtStartup},
    },
    solana_program_runtime::runtime_config::RuntimeConfig,
    solana_sdk::clock::Slot,
    std::{
        collections::HashSet,
        path::{Path, PathBuf},
        sync::Arc,
    },
};

/// Parse a `ProcessOptions` from subcommand arguments. This function attempts
/// to parse all flags related to `ProcessOptions`; however, subcommands that
/// use this function may not support all flags.
pub fn parse_process_options(ledger_path: &Path, arg_matches: &ArgMatches<'_>) -> ProcessOptions {
    let new_hard_forks = hardforks_of(arg_matches, "hard_forks");
    let accounts_db_config = Some(get_accounts_db_config(ledger_path, arg_matches));
    let runtime_config = RuntimeConfig::default();

    if arg_matches.is_present("skip_poh_verify") {
        eprintln!("--skip-poh-verify is deprecated.  Replace with --skip-verification.");
    }
    let run_verification =
        !(arg_matches.is_present("skip_poh_verify") || arg_matches.is_present("skip_verification"));
    let halt_at_slot = value_t!(arg_matches, "halt_at_slot", Slot).ok();
    let use_snapshot_archives_at_startup = value_t_or_exit!(
        arg_matches,
        use_snapshot_archives_at_startup::cli::NAME,
        UseSnapshotArchivesAtStartup
    );
    let accounts_db_skip_shrink = arg_matches.is_present("accounts_db_skip_shrink");
    let accounts_db_test_hash_calculation =
        arg_matches.is_present("accounts_db_test_hash_calculation");
    let verify_index = arg_matches.is_present("verify_accounts_index");
    let limit_load_slot_count_from_snapshot =
        value_t!(arg_matches, "limit_load_slot_count_from_snapshot", usize).ok();
    let on_halt_store_hash_raw_data_for_debug =
        arg_matches.is_present("halt_at_slot_store_hash_raw_data");
    let run_final_accounts_hash_calc = arg_matches.is_present("run_final_hash_calc");
    let debug_keys = pubkeys_of(arg_matches, "debug_key")
        .map(|pubkeys| Arc::new(pubkeys.into_iter().collect::<HashSet<_>>()));
    let allow_dead_slots = arg_matches.is_present("allow_dead_slots");

    ProcessOptions {
        new_hard_forks,
        runtime_config,
        accounts_db_config,
        accounts_db_skip_shrink,
        accounts_db_test_hash_calculation,
        verify_index,
        limit_load_slot_count_from_snapshot,
        on_halt_store_hash_raw_data_for_debug,
        run_final_accounts_hash_calc,
        debug_keys,
        run_verification,
        allow_dead_slots,
        halt_at_slot,
        use_snapshot_archives_at_startup,
        ..ProcessOptions::default()
    }
}

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
    let accounts_index_drives = values_t!(arg_matches, "accounts_index_path", String)
        .ok()
        .map(|drives| drives.into_iter().map(PathBuf::from).collect())
        .unwrap_or_else(|| vec![ledger_tool_ledger_path.join("accounts_index")]);
    let accounts_index_config = AccountsIndexConfig {
        bins: accounts_index_bins,
        index_limit_mb: accounts_index_index_limit_mb,
        drives: Some(accounts_index_drives),
        ..AccountsIndexConfig::default()
    };

    let test_partitioned_epoch_rewards =
        if arg_matches.is_present("partitioned_epoch_rewards_compare_calculation") {
            TestPartitionedEpochRewards::CompareResults
        } else if arg_matches.is_present("partitioned_epoch_rewards_force_enable_single_slot") {
            TestPartitionedEpochRewards::ForcePartitionedEpochRewardsInOneBlock
        } else {
            TestPartitionedEpochRewards::None
        };

    let accounts_hash_cache_path = arg_matches
        .value_of("accounts_hash_cache_path")
        .map(Into::into)
        .unwrap_or_else(|| {
            ledger_tool_ledger_path.join(AccountsDb::DEFAULT_ACCOUNTS_HASH_CACHE_DIR)
        });
    let accounts_hash_cache_path = create_and_canonicalize_directories([&accounts_hash_cache_path])
        .unwrap_or_else(|err| {
            eprintln!(
                "Unable to access accounts hash cache path '{}': {err}",
                accounts_hash_cache_path.display(),
            );
            std::process::exit(1);
        })
        .pop()
        .unwrap();

    AccountsDbConfig {
        index: Some(accounts_index_config),
        base_working_path: Some(ledger_tool_ledger_path),
        accounts_hash_cache_path: Some(accounts_hash_cache_path),
        ancient_append_vec_offset: value_t!(arg_matches, "accounts_db_ancient_append_vecs", i64)
            .ok(),
        exhaustively_verify_refcounts: arg_matches.is_present("accounts_db_verify_refcounts"),
        skip_initial_hash_calc: arg_matches.is_present("accounts_db_skip_initial_hash_calculation"),
        test_partitioned_epoch_rewards,
        test_skip_rewrites_but_include_in_bank_hash: arg_matches
            .is_present("accounts_db_test_skip_rewrites"),
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
