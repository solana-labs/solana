use {
    crate::LEDGER_TOOL_DIRECTORY,
    clap::{value_t, value_t_or_exit, values_t, values_t_or_exit, Arg, ArgMatches},
    solana_accounts_db::{
        accounts_db::{AccountsDb, AccountsDbConfig, CreateAncientStorage},
        accounts_file::StorageAccess,
        accounts_index::{AccountsIndexConfig, IndexLimitMb, ScanFilter},
        partitioned_rewards::TestPartitionedEpochRewards,
        utils::create_and_canonicalize_directories,
    },
    solana_clap_utils::{
        hidden_unless_forced,
        input_parsers::pubkeys_of,
        input_validators::{is_parsable, is_pow2},
    },
    solana_ledger::{
        blockstore_processor::ProcessOptions,
        use_snapshot_archives_at_startup::{self, UseSnapshotArchivesAtStartup},
    },
    solana_runtime::runtime_config::RuntimeConfig,
    solana_sdk::clock::Slot,
    std::{
        collections::HashSet,
        path::{Path, PathBuf},
        sync::Arc,
    },
};

/// Returns the arguments that configure AccountsDb
pub fn accounts_db_args<'a, 'b>() -> Box<[Arg<'a, 'b>]> {
    vec![
        Arg::with_name("account_paths")
            .long("accounts")
            .value_name("PATHS")
            .takes_value(true)
            .help(
                "Persistent accounts location. May be specified multiple times. \
                [default: <LEDGER>/accounts]",
            ),
        Arg::with_name("accounts_index_path")
            .long("accounts-index-path")
            .value_name("PATH")
            .takes_value(true)
            .multiple(true)
            .help(
                "Persistent accounts-index location. May be specified multiple times. \
                [default: <LEDGER>/accounts_index]",
            ),
        Arg::with_name("accounts_hash_cache_path")
            .long("accounts-hash-cache-path")
            .value_name("PATH")
            .takes_value(true)
            .help(
                "Use PATH as accounts hash cache location [default: <LEDGER>/accounts_hash_cache]",
            ),
        Arg::with_name("accounts_index_bins")
            .long("accounts-index-bins")
            .value_name("BINS")
            .validator(is_pow2)
            .takes_value(true)
            .help("Number of bins to divide the accounts index into"),
        Arg::with_name("disable_accounts_disk_index")
            .long("disable-accounts-disk-index")
            .help(
                "Disable the disk-based accounts index. It is enabled by default. The entire \
                 accounts index will be kept in memory.",
            ),
        Arg::with_name("accounts_db_skip_shrink")
            .long("accounts-db-skip-shrink")
            .help(
                "Enables faster starting of ledger-tool by skipping shrink. This option is for \
                use during testing.",
            ),
        Arg::with_name("accounts_db_verify_refcounts")
            .long("accounts-db-verify-refcounts")
            .help(
                "Debug option to scan all AppendVecs and verify account index refcounts prior to \
                clean",
            )
            .hidden(hidden_unless_forced()),
        Arg::with_name("accounts_db_scan_filter_for_shrinking")
            .long("accounts-db-scan-filter-for-shrinking")
            .takes_value(true)
            .possible_values(&["all", "only-abnormal", "only-abnormal-with-verify"])
            .help(
                "Debug option to use different type of filtering for accounts index scan in \
                shrinking. \"all\" will scan both in-memory and on-disk accounts index, which is the default. \
                \"only-abnormal\" will scan in-memory accounts index only for abnormal entries and \
                skip scanning on-disk accounts index by assuming that on-disk accounts index contains \
                only normal accounts index entry. \"only-abnormal-with-verify\" is similar to \
                \"only-abnormal\", which will scan in-memory index for abnormal entries, but will also \
                verify that on-disk account entries are indeed normal.",
            )
            .hidden(hidden_unless_forced()),
        Arg::with_name("accounts_db_test_skip_rewrites")
            .long("accounts-db-test-skip-rewrites")
            .help(
                "Debug option to skip rewrites for rent-exempt accounts but still add them in \
                 bank delta hash calculation",
            )
            .hidden(hidden_unless_forced()),
        Arg::with_name("accounts_db_skip_initial_hash_calculation")
            .long("accounts-db-skip-initial-hash-calculation")
            .help("Do not verify accounts hash at startup.")
            .hidden(hidden_unless_forced()),
        Arg::with_name("accounts_db_ancient_append_vecs")
            .long("accounts-db-ancient-append-vecs")
            .value_name("SLOT-OFFSET")
            .validator(is_parsable::<i64>)
            .takes_value(true)
            .help(
                "AppendVecs that are older than (slots_per_epoch - SLOT-OFFSET) are squashed \
                 together.",
            )
            .hidden(hidden_unless_forced()),
        Arg::with_name("accounts_db_squash_storages_method")
            .long("accounts-db-squash-storages-method")
            .value_name("METHOD")
            .takes_value(true)
            .possible_values(&["pack", "append"])
            .help("Squash multiple account storage files together using this method")
            .hidden(hidden_unless_forced()),
        Arg::with_name("accounts_db_access_storages_method")
            .long("accounts-db-access-storages-method")
            .value_name("METHOD")
            .takes_value(true)
            .possible_values(&["mmap", "file"])
            .help("Access account storage using this method")
            .hidden(hidden_unless_forced()),
    ]
    .into_boxed_slice()
}

// For our current version of CLAP, the value passed to Arg::default_value()
// must be a &str. But, we can't convert an integer to a &str at compile time.
// So, declare this constant and enforce equality with the following unit test
// test_max_genesis_archive_unpacked_size_constant
const MAX_GENESIS_ARCHIVE_UNPACKED_SIZE_STR: &str = "10485760";

/// Returns the arguments that configure loading genesis
pub fn load_genesis_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("max_genesis_archive_unpacked_size")
        .long("max-genesis-archive-unpacked-size")
        .value_name("NUMBER")
        .takes_value(true)
        .default_value(MAX_GENESIS_ARCHIVE_UNPACKED_SIZE_STR)
        .help("maximum total uncompressed size of unpacked genesis archive")
}

/// Returns the arguments that configure snapshot loading
pub fn snapshot_args<'a, 'b>() -> Box<[Arg<'a, 'b>]> {
    vec![
        Arg::with_name("no_snapshot")
            .long("no-snapshot")
            .takes_value(false)
            .help("Do not start from a local snapshot if present"),
        Arg::with_name("snapshots")
            .long("snapshots")
            .alias("snapshot-archive-path")
            .alias("full-snapshot-archive-path")
            .value_name("DIR")
            .takes_value(true)
            .global(true)
            .help("Use DIR for snapshot location [default: --ledger value]"),
        Arg::with_name("incremental_snapshot_archive_path")
            .long("incremental-snapshot-archive-path")
            .value_name("DIR")
            .takes_value(true)
            .global(true)
            .help("Use DIR for separate incremental snapshot location"),
        Arg::with_name(use_snapshot_archives_at_startup::cli::NAME)
            .long(use_snapshot_archives_at_startup::cli::LONG_ARG)
            .takes_value(true)
            .possible_values(use_snapshot_archives_at_startup::cli::POSSIBLE_VALUES)
            .default_value(use_snapshot_archives_at_startup::cli::default_value_for_ledger_tool())
            .help(use_snapshot_archives_at_startup::cli::HELP)
            .long_help(use_snapshot_archives_at_startup::cli::LONG_HELP),
    ]
    .into_boxed_slice()
}

/// Parse a `ProcessOptions` from subcommand arguments. This function attempts
/// to parse all flags related to `ProcessOptions`; however, subcommands that
/// use this function may not support all flags.
pub fn parse_process_options(ledger_path: &Path, arg_matches: &ArgMatches<'_>) -> ProcessOptions {
    let new_hard_forks = hardforks_of(arg_matches, "hard_forks");
    let accounts_db_config = Some(get_accounts_db_config(ledger_path, arg_matches));
    let log_messages_bytes_limit = value_t!(arg_matches, "log_messages_bytes_limit", usize).ok();
    let runtime_config = RuntimeConfig {
        log_messages_bytes_limit,
        ..RuntimeConfig::default()
    };

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
    let abort_on_invalid_block = arg_matches.is_present("abort_on_invalid_block");
    let no_block_cost_limits = arg_matches.is_present("no_block_cost_limits");

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
        abort_on_invalid_block,
        no_block_cost_limits,
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
    let accounts_index_index_limit_mb = if arg_matches.is_present("disable_accounts_disk_index") {
        IndexLimitMb::InMemOnly
    } else {
        IndexLimitMb::Unlimited
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

    let create_ancient_storage = arg_matches
        .value_of("accounts_db_squash_storages_method")
        .map(|method| match method {
            "pack" => CreateAncientStorage::Pack,
            "append" => CreateAncientStorage::Append,
            _ => {
                // clap will enforce one of the above values is given
                unreachable!("invalid value given to accounts-db-squash-storages-method")
            }
        })
        .unwrap_or_default();
    let storage_access = arg_matches
        .value_of("accounts_db_access_storages_method")
        .map(|method| match method {
            "mmap" => StorageAccess::Mmap,
            "file" => StorageAccess::File,
            _ => {
                // clap will enforce one of the above values is given
                unreachable!("invalid value given to accounts-db-access-storages-method")
            }
        })
        .unwrap_or_default();

    let scan_filter_for_shrinking = arg_matches
        .value_of("accounts_db_scan_filter_for_shrinking")
        .map(|filter| match filter {
            "all" => ScanFilter::All,
            "only-abnormal" => ScanFilter::OnlyAbnormal,
            "only-abnormal-with-verify" => ScanFilter::OnlyAbnormalWithVerify,
            _ => {
                // clap will enforce one of the above values is given
                unreachable!("invalid value given to accounts_db_scan_filter_for_shrinking")
            }
        })
        .unwrap_or_default();

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
        create_ancient_storage,
        storage_access,
        scan_filter_for_shrinking,
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

#[cfg(test)]
mod tests {
    use {super::*, solana_accounts_db::hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE};

    #[test]
    fn test_max_genesis_archive_unpacked_size_constant() {
        assert_eq!(
            MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
            MAX_GENESIS_ARCHIVE_UNPACKED_SIZE_STR
                .parse::<u64>()
                .unwrap()
        );
    }
}
