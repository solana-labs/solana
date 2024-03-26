#![allow(clippy::arithmetic_side_effects)]
use {
    crate::{
        args::*,
        bigtable::*,
        blockstore::*,
        ledger_path::*,
        ledger_utils::*,
        output::{
            output_account, AccountsOutputConfig, AccountsOutputMode, AccountsOutputStreamer,
        },
        program::*,
    },
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, values_t_or_exit, App,
        AppSettings, Arg, ArgMatches, SubCommand,
    },
    dashmap::DashMap,
    log::*,
    serde::Serialize,
    solana_account_decoder::UiAccountEncoding,
    solana_accounts_db::{
        accounts_db::CalcAccountsHashDataSource, accounts_index::ScanConfig,
        hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
    },
    solana_clap_utils::{
        hidden_unless_forced,
        input_parsers::{cluster_type_of, pubkey_of, pubkeys_of},
        input_validators::{
            is_parsable, is_pow2, is_pubkey, is_pubkey_or_keypair, is_slot, is_valid_percentage,
            is_within_range, validate_maximum_full_snapshot_archives_to_retain,
            validate_maximum_incremental_snapshot_archives_to_retain,
        },
    },
    solana_cli_output::OutputFormat,
    solana_core::{
        system_monitor_service::{SystemMonitorService, SystemMonitorStatsReportConfig},
        validator::BlockVerificationMethod,
    },
    solana_cost_model::{cost_model::CostModel, cost_tracker::CostTracker},
    solana_ledger::{
        blockstore::{create_new_ledger, Blockstore},
        blockstore_options::{AccessType, LedgerColumnOptions},
        blockstore_processor::ProcessSlotCallback,
        use_snapshot_archives_at_startup,
    },
    solana_measure::{measure, measure::Measure},
    solana_runtime::{
        bank::{bank_hash_details, Bank, RewardCalculationEvent},
        bank_forks::BankForks,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_bank_utils,
        snapshot_minimizer::SnapshotMinimizer,
        snapshot_utils::{
            ArchiveFormat, SnapshotVersion, DEFAULT_ARCHIVE_COMPRESSION,
            SUPPORTED_ARCHIVE_COMPRESSION,
        },
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        account_utils::StateMut,
        clock::{Epoch, Slot},
        feature::{self, Feature},
        feature_set::{self, FeatureSet},
        genesis_config::ClusterType,
        inflation::Inflation,
        native_token::{lamports_to_sol, sol_to_lamports, Sol},
        pubkey::Pubkey,
        rent::Rent,
        shred_version::compute_shred_version,
        stake::{self, state::StakeStateV2},
        system_program,
        transaction::{MessageHash, SanitizedTransaction, SimpleAddressLoader},
    },
    solana_stake_program::{points::PointValue, stake_state},
    solana_unified_scheduler_pool::DefaultSchedulerPool,
    solana_vote_program::{
        self,
        vote_state::{self, VoteState},
    },
    std::{
        collections::{HashMap, HashSet},
        ffi::OsStr,
        fs::File,
        io::{self, Write},
        num::NonZeroUsize,
        path::{Path, PathBuf},
        process::{exit, Command, Stdio},
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex, RwLock,
        },
    },
};

mod args;
mod bigtable;
mod blockstore;
mod error;
mod ledger_path;
mod ledger_utils;
mod output;
mod program;

fn parse_encoding_format(matches: &ArgMatches<'_>) -> UiAccountEncoding {
    match matches.value_of("encoding") {
        Some("jsonParsed") => UiAccountEncoding::JsonParsed,
        Some("base64") => UiAccountEncoding::Base64,
        Some("base64+zstd") => UiAccountEncoding::Base64Zstd,
        _ => UiAccountEncoding::Base64,
    }
}

fn render_dot(dot: String, output_file: &str, output_format: &str) -> io::Result<()> {
    let mut child = Command::new("dot")
        .arg(format!("-T{output_format}"))
        .arg(format!("-o{output_file}"))
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| {
            eprintln!("Failed to spawn dot: {err:?}");
            err
        })?;

    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(&dot.into_bytes())?;

    let status = child.wait_with_output()?.status;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("dot failed with error {}", status.code().unwrap_or(-1)),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum GraphVoteAccountMode {
    Disabled,
    LastOnly,
    WithHistory,
}

impl GraphVoteAccountMode {
    const DISABLED: &'static str = "disabled";
    const LAST_ONLY: &'static str = "last-only";
    const WITH_HISTORY: &'static str = "with-history";
    const ALL_MODE_STRINGS: &'static [&'static str] =
        &[Self::DISABLED, Self::LAST_ONLY, Self::WITH_HISTORY];

    fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

impl AsRef<str> for GraphVoteAccountMode {
    fn as_ref(&self) -> &str {
        match self {
            Self::Disabled => Self::DISABLED,
            Self::LastOnly => Self::LAST_ONLY,
            Self::WithHistory => Self::WITH_HISTORY,
        }
    }
}

impl Default for GraphVoteAccountMode {
    fn default() -> Self {
        Self::Disabled
    }
}

struct GraphVoteAccountModeError(String);

impl FromStr for GraphVoteAccountMode {
    type Err = GraphVoteAccountModeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            Self::DISABLED => Ok(Self::Disabled),
            Self::LAST_ONLY => Ok(Self::LastOnly),
            Self::WITH_HISTORY => Ok(Self::WithHistory),
            _ => Err(GraphVoteAccountModeError(s.to_string())),
        }
    }
}

struct GraphConfig {
    include_all_votes: bool,
    vote_account_mode: GraphVoteAccountMode,
}

#[allow(clippy::cognitive_complexity)]
fn graph_forks(bank_forks: &BankForks, config: &GraphConfig) -> String {
    let frozen_banks = bank_forks.frozen_banks();
    let mut fork_slots: HashSet<_> = frozen_banks.keys().cloned().collect();
    for (_, bank) in frozen_banks {
        for parent in bank.parents() {
            fork_slots.remove(&parent.slot());
        }
    }

    // Search all forks and collect the last vote made by each validator
    let mut last_votes = HashMap::new();
    let default_vote_state = VoteState::default();
    for fork_slot in &fork_slots {
        let bank = &bank_forks[*fork_slot];

        let total_stake = bank
            .vote_accounts()
            .iter()
            .map(|(_, (stake, _))| stake)
            .sum();
        for (stake, vote_account) in bank.vote_accounts().values() {
            let vote_state = vote_account.vote_state();
            let vote_state = vote_state.unwrap_or(&default_vote_state);
            if let Some(last_vote) = vote_state.votes.iter().last() {
                let entry = last_votes.entry(vote_state.node_pubkey).or_insert((
                    last_vote.slot(),
                    vote_state.clone(),
                    *stake,
                    total_stake,
                ));
                if entry.0 < last_vote.slot() {
                    *entry = (last_vote.slot(), vote_state.clone(), *stake, total_stake);
                }
            }
        }
    }

    // Figure the stake distribution at all the nodes containing the last vote from each
    // validator
    let mut slot_stake_and_vote_count = HashMap::new();
    for (last_vote_slot, _, stake, total_stake) in last_votes.values() {
        let entry = slot_stake_and_vote_count
            .entry(last_vote_slot)
            .or_insert((0, 0, *total_stake));
        entry.0 += 1;
        entry.1 += stake;
        assert_eq!(entry.2, *total_stake)
    }

    let mut dot = vec!["digraph {".to_string()];

    // Build a subgraph consisting of all banks and links to their parent banks
    dot.push("  subgraph cluster_banks {".to_string());
    dot.push("    style=invis".to_string());
    let mut styled_slots = HashSet::new();
    let mut all_votes: HashMap<Pubkey, HashMap<Slot, VoteState>> = HashMap::new();
    for fork_slot in &fork_slots {
        let mut bank = bank_forks[*fork_slot].clone();

        let mut first = true;
        loop {
            for (_, vote_account) in bank.vote_accounts().values() {
                let vote_state = vote_account.vote_state();
                let vote_state = vote_state.unwrap_or(&default_vote_state);
                if let Some(last_vote) = vote_state.votes.iter().last() {
                    let validator_votes = all_votes.entry(vote_state.node_pubkey).or_default();
                    validator_votes
                        .entry(last_vote.slot())
                        .or_insert_with(|| vote_state.clone());
                }
            }

            if !styled_slots.contains(&bank.slot()) {
                dot.push(format!(
                    r#"    "{}"[label="{} (epoch {})\nleader: {}{}{}",style="{}{}"];"#,
                    bank.slot(),
                    bank.slot(),
                    bank.epoch(),
                    bank.collector_id(),
                    if let Some(parent) = bank.parent() {
                        format!(
                            "\ntransactions: {}",
                            bank.transaction_count() - parent.transaction_count(),
                        )
                    } else {
                        "".to_string()
                    },
                    if let Some((votes, stake, total_stake)) =
                        slot_stake_and_vote_count.get(&bank.slot())
                    {
                        format!(
                            "\nvotes: {}, stake: {:.1} SOL ({:.1}%)",
                            votes,
                            lamports_to_sol(*stake),
                            *stake as f64 / *total_stake as f64 * 100.,
                        )
                    } else {
                        "".to_string()
                    },
                    if first { "filled," } else { "" },
                    ""
                ));
                styled_slots.insert(bank.slot());
            }
            first = false;

            match bank.parent() {
                None => {
                    if bank.slot() > 0 {
                        dot.push(format!(r#"    "{}" -> "..." [dir=back]"#, bank.slot(),));
                    }
                    break;
                }
                Some(parent) => {
                    let slot_distance = bank.slot() - parent.slot();
                    let penwidth = if bank.epoch() > parent.epoch() {
                        "5"
                    } else {
                        "1"
                    };
                    let link_label = if slot_distance > 1 {
                        format!("label=\"{} slots\",color=red", slot_distance - 1)
                    } else {
                        "color=blue".to_string()
                    };
                    dot.push(format!(
                        r#"    "{}" -> "{}"[{},dir=back,penwidth={}];"#,
                        bank.slot(),
                        parent.slot(),
                        link_label,
                        penwidth
                    ));

                    bank = parent.clone();
                }
            }
        }
    }
    dot.push("  }".to_string());

    // Strafe the banks with links from validators to the bank they last voted on,
    // while collecting information about the absent votes and stakes
    let mut absent_stake = 0;
    let mut absent_votes = 0;
    let mut lowest_last_vote_slot = std::u64::MAX;
    let mut lowest_total_stake = 0;
    for (node_pubkey, (last_vote_slot, vote_state, stake, total_stake)) in &last_votes {
        all_votes.entry(*node_pubkey).and_modify(|validator_votes| {
            validator_votes.remove(last_vote_slot);
        });

        let maybe_styled_last_vote_slot = styled_slots.get(last_vote_slot);
        if maybe_styled_last_vote_slot.is_none() {
            if *last_vote_slot < lowest_last_vote_slot {
                lowest_last_vote_slot = *last_vote_slot;
                lowest_total_stake = *total_stake;
            }
            absent_votes += 1;
            absent_stake += stake;
        };

        if config.vote_account_mode.is_enabled() {
            let vote_history =
                if matches!(config.vote_account_mode, GraphVoteAccountMode::WithHistory) {
                    format!(
                        "vote history:\n{}",
                        vote_state
                            .votes
                            .iter()
                            .map(|vote| format!(
                                "slot {} (conf={})",
                                vote.slot(),
                                vote.confirmation_count()
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                } else {
                    format!(
                        "last vote slot: {}",
                        vote_state
                            .votes
                            .back()
                            .map(|vote| vote.slot().to_string())
                            .unwrap_or_else(|| "none".to_string())
                    )
                };
            dot.push(format!(
                r#"  "last vote {}"[shape=box,label="Latest validator vote: {}\nstake: {} SOL\nroot slot: {}\n{}"];"#,
                node_pubkey,
                node_pubkey,
                lamports_to_sol(*stake),
                vote_state.root_slot.unwrap_or(0),
                vote_history,
            ));

            dot.push(format!(
                r#"  "last vote {}" -> "{}" [style=dashed,label="latest vote"];"#,
                node_pubkey,
                if let Some(styled_last_vote_slot) = maybe_styled_last_vote_slot {
                    styled_last_vote_slot.to_string()
                } else {
                    "...".to_string()
                },
            ));
        }
    }

    // Annotate the final "..." node with absent vote and stake information
    if absent_votes > 0 {
        dot.push(format!(
            r#"    "..."[label="...\nvotes: {}, stake: {:.1} SOL {:.1}%"];"#,
            absent_votes,
            lamports_to_sol(absent_stake),
            absent_stake as f64 / lowest_total_stake as f64 * 100.,
        ));
    }

    // Add for vote information from all banks.
    if config.include_all_votes {
        for (node_pubkey, validator_votes) in &all_votes {
            for (vote_slot, vote_state) in validator_votes {
                dot.push(format!(
                    r#"  "{} vote {}"[shape=box,style=dotted,label="validator vote: {}\nroot slot: {}\nvote history:\n{}"];"#,
                    node_pubkey,
                    vote_slot,
                    node_pubkey,
                    vote_state.root_slot.unwrap_or(0),
                    vote_state
                        .votes
                        .iter()
                        .map(|vote| format!("slot {} (conf={})", vote.slot(), vote.confirmation_count()))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));

                dot.push(format!(
                    r#"  "{} vote {}" -> "{}" [style=dotted,label="vote"];"#,
                    node_pubkey,
                    vote_slot,
                    if styled_slots.contains(vote_slot) {
                        vote_slot.to_string()
                    } else {
                        "...".to_string()
                    },
                ));
            }
        }
    }

    dot.push("}".to_string());
    dot.join("\n")
}

fn compute_slot_cost(
    blockstore: &Blockstore,
    slot: Slot,
    allow_dead_slots: bool,
) -> Result<(), String> {
    let (entries, _num_shreds, _is_full) = blockstore
        .get_slot_entries_with_shred_info(slot, 0, allow_dead_slots)
        .map_err(|err| format!("Slot: {slot}, Failed to load entries, err {err:?}"))?;

    let num_entries = entries.len();
    let mut num_transactions = 0;
    let mut num_programs = 0;

    let mut program_ids = HashMap::new();
    let mut cost_tracker = CostTracker::default();

    for entry in entries {
        num_transactions += entry.transactions.len();
        entry
            .transactions
            .into_iter()
            .filter_map(|transaction| {
                SanitizedTransaction::try_create(
                    transaction,
                    MessageHash::Compute,
                    None,
                    SimpleAddressLoader::Disabled,
                )
                .map_err(|err| {
                    warn!("Failed to compute cost of transaction: {:?}", err);
                })
                .ok()
            })
            .for_each(|transaction| {
                num_programs += transaction.message().instructions().len();

                let tx_cost = CostModel::calculate_cost(&transaction, &FeatureSet::all_enabled());
                let result = cost_tracker.try_add(&tx_cost);
                if result.is_err() {
                    println!(
                        "Slot: {slot}, CostModel rejected transaction {transaction:?}, reason \
                         {result:?}",
                    );
                }
                for (program_id, _instruction) in transaction.message().program_instructions_iter()
                {
                    *program_ids.entry(*program_id).or_insert(0) += 1;
                }
            });
    }

    println!(
        "Slot: {slot}, Entries: {num_entries}, Transactions: {num_transactions}, Programs \
         {num_programs}",
    );
    println!("  Programs: {program_ids:?}");

    Ok(())
}

/// Finds the accounts needed to replay slots `snapshot_slot` to `ending_slot`.
/// Removes all other accounts from accounts_db, and updates the accounts hash
/// and capitalization. This is used by the --minimize option in create-snapshot
/// Returns true if the minimized snapshot may be incomplete.
fn minimize_bank_for_snapshot(
    blockstore: &Blockstore,
    bank: &Bank,
    snapshot_slot: Slot,
    ending_slot: Slot,
) -> bool {
    let ((transaction_account_set, possibly_incomplete), transaction_accounts_measure) = measure!(
        blockstore.get_accounts_used_in_range(bank, snapshot_slot, ending_slot),
        "get transaction accounts"
    );
    let total_accounts_len = transaction_account_set.len();
    info!("Added {total_accounts_len} accounts from transactions. {transaction_accounts_measure}");

    SnapshotMinimizer::minimize(bank, snapshot_slot, ending_slot, transaction_account_set);
    possibly_incomplete
}

fn assert_capitalization(bank: &Bank) {
    let debug_verify = true;
    assert!(bank.calculate_and_verify_capitalization(debug_verify));
}

#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[allow(clippy::cognitive_complexity)]
fn main() {
    // Ignore SIGUSR1 to prevent long-running calls being killed by logrotate
    // in warehouse deployments
    #[cfg(unix)]
    {
        // `register()` is unsafe because the action is called in a signal handler
        // with the usual caveats. So long as this action body stays empty, we'll
        // be fine
        unsafe { signal_hook::low_level::register(signal_hook::consts::SIGUSR1, || {}) }.unwrap();
    }

    // Use std::usize::MAX for DEFAULT_MAX_*_SNAPSHOTS_TO_RETAIN such that
    // ledger-tool commands won't accidentally remove any snapshots by default
    const DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN: usize = std::usize::MAX;
    const DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN: usize = std::usize::MAX;

    solana_logger::setup_with_default("solana=info");

    let no_snapshot_arg = Arg::with_name("no_snapshot")
        .long("no-snapshot")
        .takes_value(false)
        .help("Do not start from a local snapshot if present");
    let accounts_index_bins = Arg::with_name("accounts_index_bins")
        .long("accounts-index-bins")
        .value_name("BINS")
        .validator(is_pow2)
        .takes_value(true)
        .help("Number of bins to divide the accounts index into");
    let accounts_index_limit = Arg::with_name("accounts_index_memory_limit_mb")
        .long("accounts-index-memory-limit-mb")
        .value_name("MEGABYTES")
        .validator(is_parsable::<usize>)
        .takes_value(true)
        .help(
            "How much memory the accounts index can consume. If this is exceeded, some account \
             index entries will be stored on disk.",
        );
    let disable_disk_index = Arg::with_name("disable_accounts_disk_index")
        .long("disable-accounts-disk-index")
        .help(
            "Disable the disk-based accounts index. It is enabled by default. The entire accounts \
             index will be kept in memory.",
        )
        .conflicts_with("accounts_index_memory_limit_mb");
    let accountsdb_skip_shrink = Arg::with_name("accounts_db_skip_shrink")
        .long("accounts-db-skip-shrink")
        .help(
            "Enables faster starting of ledger-tool by skipping shrink. This option is for use \
             during testing.",
        );
    let accountsdb_verify_refcounts = Arg::with_name("accounts_db_verify_refcounts")
        .long("accounts-db-verify-refcounts")
        .help(
            "Debug option to scan all AppendVecs and verify account index refcounts prior to clean",
        )
        .hidden(hidden_unless_forced());
    let accounts_db_test_skip_rewrites_but_include_in_bank_hash =
        Arg::with_name("accounts_db_test_skip_rewrites")
            .long("accounts-db-test-skip-rewrites")
            .help(
                "Debug option to skip rewrites for rent-exempt accounts but still add them in \
                 bank delta hash calculation",
            )
            .hidden(hidden_unless_forced());
    let account_paths_arg = Arg::with_name("account_paths")
        .long("accounts")
        .value_name("PATHS")
        .takes_value(true)
        .help(
            "Persistent accounts location. \
            May be specified multiple times. \
            [default: <LEDGER>/accounts]",
        );
    let accounts_hash_cache_path_arg = Arg::with_name("accounts_hash_cache_path")
        .long("accounts-hash-cache-path")
        .value_name("PATH")
        .takes_value(true)
        .help("Use PATH as accounts hash cache location [default: <LEDGER>/accounts_hash_cache]");
    let accounts_index_path_arg = Arg::with_name("accounts_index_path")
        .long("accounts-index-path")
        .value_name("PATH")
        .takes_value(true)
        .multiple(true)
        .help(
            "Persistent accounts-index location. \
            May be specified multiple times. \
            [default: <LEDGER>/accounts_index]",
        );
    let accounts_db_test_hash_calculation_arg = Arg::with_name("accounts_db_test_hash_calculation")
        .long("accounts-db-test-hash-calculation")
        .help("Enable hash calculation test");
    let halt_at_slot_arg = Arg::with_name("halt_at_slot")
        .long("halt-at-slot")
        .value_name("SLOT")
        .validator(is_slot)
        .takes_value(true)
        .help("Halt processing at the given slot");
    let os_memory_stats_reporting_arg = Arg::with_name("os_memory_stats_reporting")
        .long("os-memory-stats-reporting")
        .help("Enable reporting of OS memory statistics.");
    let accounts_db_skip_initial_hash_calc_arg =
        Arg::with_name("accounts_db_skip_initial_hash_calculation")
            .long("accounts-db-skip-initial-hash-calculation")
            .help("Do not verify accounts hash at startup.")
            .hidden(hidden_unless_forced());
    let ancient_append_vecs = Arg::with_name("accounts_db_ancient_append_vecs")
        .long("accounts-db-ancient-append-vecs")
        .value_name("SLOT-OFFSET")
        .validator(is_parsable::<i64>)
        .takes_value(true)
        .help(
            "AppendVecs that are older than (slots_per_epoch - SLOT-OFFSET) are squashed together.",
        )
        .hidden(hidden_unless_forced());
    let halt_at_slot_store_hash_raw_data = Arg::with_name("halt_at_slot_store_hash_raw_data")
        .long("halt-at-slot-store-hash-raw-data")
        .help(
            "After halting at slot, run an accounts hash calculation and store the raw hash data \
             for debugging.",
        )
        .hidden(hidden_unless_forced());
    let verify_index_arg = Arg::with_name("verify_accounts_index")
        .long("verify-accounts-index")
        .takes_value(false)
        .help("For debugging and tests on accounts index.");
    let limit_load_slot_count_from_snapshot_arg =
        Arg::with_name("limit_load_slot_count_from_snapshot")
            .long("limit-load-slot-count-from-snapshot")
            .value_name("SLOT")
            .validator(is_slot)
            .takes_value(true)
            .help(
                "For debugging and profiling with large snapshots, artificially limit how many \
                 slots are loaded from a snapshot.",
            );
    let hard_forks_arg = Arg::with_name("hard_forks")
        .long("hard-fork")
        .value_name("SLOT")
        .validator(is_slot)
        .multiple(true)
        .takes_value(true)
        .help("Add a hard fork at this slot");
    let allow_dead_slots_arg = Arg::with_name("allow_dead_slots")
        .long("allow-dead-slots")
        .takes_value(false)
        .help("Output dead slots as well");
    let default_genesis_archive_unpacked_size = MAX_GENESIS_ARCHIVE_UNPACKED_SIZE.to_string();
    let max_genesis_archive_unpacked_size_arg = Arg::with_name("max_genesis_archive_unpacked_size")
        .long("max-genesis-archive-unpacked-size")
        .value_name("NUMBER")
        .takes_value(true)
        .default_value(&default_genesis_archive_unpacked_size)
        .help("maximum total uncompressed size of unpacked genesis archive");
    let hashes_per_tick = Arg::with_name("hashes_per_tick")
        .long("hashes-per-tick")
        .value_name("NUM_HASHES|\"sleep\"")
        .takes_value(true)
        .help(
            "How many PoH hashes to roll before emitting the next tick. If \"sleep\", for \
             development sleep for the target tick duration instead of hashing",
        );
    let snapshot_version_arg = Arg::with_name("snapshot_version")
        .long("snapshot-version")
        .value_name("SNAPSHOT_VERSION")
        .validator(is_parsable::<SnapshotVersion>)
        .takes_value(true)
        .default_value(SnapshotVersion::default().into())
        .help("Output snapshot version");
    let debug_key_arg = Arg::with_name("debug_key")
        .long("debug-key")
        .validator(is_pubkey)
        .value_name("ADDRESS")
        .multiple(true)
        .takes_value(true)
        .help("Log when transactions are processed that reference the given key(s).");
    let use_snapshot_archives_at_startup =
        Arg::with_name(use_snapshot_archives_at_startup::cli::NAME)
            .long(use_snapshot_archives_at_startup::cli::LONG_ARG)
            .takes_value(true)
            .possible_values(use_snapshot_archives_at_startup::cli::POSSIBLE_VALUES)
            .default_value(use_snapshot_archives_at_startup::cli::default_value_for_ledger_tool())
            .help(use_snapshot_archives_at_startup::cli::HELP)
            .long_help(use_snapshot_archives_at_startup::cli::LONG_HELP);

    let default_max_full_snapshot_archives_to_retain =
        &DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN.to_string();
    let maximum_full_snapshot_archives_to_retain =
        Arg::with_name("maximum_full_snapshots_to_retain")
            .long("maximum-full-snapshots-to-retain")
            .alias("maximum-snapshots-to-retain")
            .value_name("NUMBER")
            .takes_value(true)
            .default_value(default_max_full_snapshot_archives_to_retain)
            .validator(validate_maximum_full_snapshot_archives_to_retain)
            .help(
                "The maximum number of full snapshot archives to hold on to when purging older \
                 snapshots.",
            );

    let default_max_incremental_snapshot_archives_to_retain =
        &DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN.to_string();
    let maximum_incremental_snapshot_archives_to_retain =
        Arg::with_name("maximum_incremental_snapshots_to_retain")
            .long("maximum-incremental-snapshots-to-retain")
            .value_name("NUMBER")
            .takes_value(true)
            .default_value(default_max_incremental_snapshot_archives_to_retain)
            .validator(validate_maximum_incremental_snapshot_archives_to_retain)
            .help(
                "The maximum number of incremental snapshot archives to hold on to when purging \
                 older snapshots.",
            );

    let geyser_plugin_args = Arg::with_name("geyser_plugin_config")
        .long("geyser-plugin-config")
        .value_name("FILE")
        .takes_value(true)
        .multiple(true)
        .help("Specify the configuration file for the Geyser plugin.");

    let accounts_data_encoding_arg = Arg::with_name("encoding")
        .long("encoding")
        .takes_value(true)
        .possible_values(&["base64", "base64+zstd", "jsonParsed"])
        .default_value("base64")
        .help("Print account data in specified format when printing account contents.");

    let rent = Rent::default();
    let default_bootstrap_validator_lamports = &sol_to_lamports(500.0)
        .max(VoteState::get_rent_exempt_reserve(&rent))
        .to_string();
    let default_bootstrap_validator_stake_lamports = &sol_to_lamports(0.5)
        .max(rent.minimum_balance(StakeStateV2::size_of()))
        .to_string();
    let default_graph_vote_account_mode = GraphVoteAccountMode::default();

    let mut measure_total_execution_time = Measure::start("ledger tool");

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::InferSubcommands)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::VersionlessSubcommands)
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .default_value("ledger")
                .help("Use DIR as ledger location"),
        )
        .arg(
            Arg::with_name("wal_recovery_mode")
                .long("wal-recovery-mode")
                .value_name("MODE")
                .takes_value(true)
                .global(true)
                .possible_values(&[
                    "tolerate_corrupted_tail_records",
                    "absolute_consistency",
                    "point_in_time",
                    "skip_any_corrupted_record",
                ])
                .help("Mode to recovery the ledger db write ahead log"),
        )
        .arg(
            Arg::with_name("force_update_to_open")
                .long("force-update-to-open")
                .takes_value(false)
                .global(true)
                .help(
                    "Allow commands that would otherwise not alter the blockstore to make \
                     necessary updates in order to open it",
                ),
        )
        .arg(
            Arg::with_name("ignore_ulimit_nofile_error")
                .long("ignore-ulimit-nofile-error")
                .takes_value(false)
                .global(true)
                .help(
                    "Allow opening the blockstore to succeed even if the desired open file \
                     descriptor limit cannot be configured. Use with caution as some commands may \
                     run fine with a reduced file descriptor limit while others will not",
                ),
        )
        .arg(
            Arg::with_name("snapshots")
                .long("snapshots")
                .alias("snapshot-archive-path")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .help("Use DIR for snapshot location [default: --ledger value]"),
        )
        .arg(
            Arg::with_name("incremental_snapshot_archive_path")
                .long("incremental-snapshot-archive-path")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .help("Use DIR for separate incremental snapshot location"),
        )
        .arg(
            Arg::with_name("block_verification_method")
                .long("block-verification-method")
                .value_name("METHOD")
                .takes_value(true)
                .possible_values(BlockVerificationMethod::cli_names())
                .global(true)
                .hidden(hidden_unless_forced())
                .help(BlockVerificationMethod::cli_message()),
        )
        .arg(
            Arg::with_name("unified_scheduler_handler_threads")
                .long("unified-scheduler-handler-threads")
                .value_name("COUNT")
                .takes_value(true)
                .validator(|s| is_within_range(s, 1..))
                .global(true)
                .hidden(hidden_unless_forced())
                .help(DefaultSchedulerPool::cli_message()),
        )
        .arg(
            Arg::with_name("output_format")
                .long("output")
                .value_name("FORMAT")
                .global(true)
                .takes_value(true)
                .possible_values(&["json", "json-compact"])
                .help(
                    "Return information in specified output format, currently only available for \
                     bigtable and program subcommands",
                ),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .global(true)
                .multiple(true)
                .takes_value(false)
                .help("Show additional information where supported"),
        )
        .bigtable_subcommand()
        .blockstore_subcommand()
        // All of the blockstore commands are added under the blockstore command.
        // For the sake of legacy support, also directly add the blockstore commands here so that
        // these subcommands can continue to be called from the top level of the binary.
        .subcommands(blockstore_subcommands(true))
        .subcommand(
            SubCommand::with_name("genesis")
                .about("Prints the ledger's genesis config")
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(
                    Arg::with_name("accounts")
                        .long("accounts")
                        .takes_value(false)
                        .help("Print the ledger's genesis accounts"),
                )
                .arg(
                    Arg::with_name("no_account_data")
                        .long("no-account-data")
                        .takes_value(false)
                        .requires("accounts")
                        .help("Do not print account data when printing account contents."),
                )
                .arg(&accounts_data_encoding_arg),
        )
        .subcommand(
            SubCommand::with_name("genesis-hash")
                .about("Prints the ledger's genesis hash")
                .arg(&max_genesis_archive_unpacked_size_arg),
        )
        .subcommand(
            SubCommand::with_name("modify-genesis")
                .about("Modifies genesis parameters")
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(&hashes_per_tick)
                .arg(
                    Arg::with_name("cluster_type")
                        .long("cluster-type")
                        .possible_values(&ClusterType::STRINGS)
                        .takes_value(true)
                        .help("Selects the features that will be enabled for the cluster"),
                )
                .arg(
                    Arg::with_name("output_directory")
                        .index(1)
                        .value_name("DIR")
                        .takes_value(true)
                        .help("Output directory for the modified genesis config"),
                ),
        )
        .subcommand(
            SubCommand::with_name("shred-version")
                .about("Prints the ledger's shred hash")
                .arg(&hard_forks_arg)
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(&accounts_index_bins)
                .arg(&accounts_index_limit)
                .arg(&disable_disk_index)
                .arg(&accountsdb_verify_refcounts)
                .arg(&accounts_db_skip_initial_hash_calc_arg)
                .arg(&accounts_db_test_skip_rewrites_but_include_in_bank_hash)
                .arg(&use_snapshot_archives_at_startup),
        )
        .subcommand(
            SubCommand::with_name("bank-hash")
                .about("Prints the hash of the working bank after reading the ledger")
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(&halt_at_slot_arg)
                .arg(&accounts_index_bins)
                .arg(&accounts_index_limit)
                .arg(&disable_disk_index)
                .arg(&accountsdb_verify_refcounts)
                .arg(&accounts_db_skip_initial_hash_calc_arg)
                .arg(&accounts_db_test_skip_rewrites_but_include_in_bank_hash)
                .arg(&use_snapshot_archives_at_startup),
        )
        .subcommand(
            SubCommand::with_name("verify")
                .about("Verify the ledger")
                .arg(&no_snapshot_arg)
                .arg(&account_paths_arg)
                .arg(&accounts_hash_cache_path_arg)
                .arg(&accounts_index_path_arg)
                .arg(&halt_at_slot_arg)
                .arg(&limit_load_slot_count_from_snapshot_arg)
                .arg(&accounts_index_bins)
                .arg(&accounts_index_limit)
                .arg(&disable_disk_index)
                .arg(&accountsdb_skip_shrink)
                .arg(&accountsdb_verify_refcounts)
                .arg(&accounts_db_test_skip_rewrites_but_include_in_bank_hash)
                .arg(&verify_index_arg)
                .arg(&accounts_db_skip_initial_hash_calc_arg)
                .arg(&ancient_append_vecs)
                .arg(&halt_at_slot_store_hash_raw_data)
                .arg(&hard_forks_arg)
                .arg(&accounts_db_test_hash_calculation_arg)
                .arg(&os_memory_stats_reporting_arg)
                .arg(&allow_dead_slots_arg)
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(&debug_key_arg)
                .arg(&geyser_plugin_args)
                .arg(&use_snapshot_archives_at_startup)
                .arg(
                    Arg::with_name("skip_poh_verify")
                        .long("skip-poh-verify")
                        .takes_value(false)
                        .help(
                            "Deprecated, please use --skip-verification. Skip ledger PoH and \
                             transaction verification.",
                        ),
                )
                .arg(
                    Arg::with_name("skip_verification")
                        .long("skip-verification")
                        .takes_value(false)
                        .help("Skip ledger PoH and transaction verification."),
                )
                .arg(
                    Arg::with_name("enable_rpc_transaction_history")
                        .long("enable-rpc-transaction-history")
                        .takes_value(false)
                        .help("Store transaction info for processed slots into local ledger"),
                )
                .arg(
                    Arg::with_name("run_final_hash_calc")
                        .long("run-final-accounts-hash-calculation")
                        .takes_value(false)
                        .help(
                            "After 'verify' completes, run a final accounts hash calculation. \
                             Final hash calculation could race with accounts background service \
                             tasks and assert.",
                        ),
                )
                .arg(
                    Arg::with_name("partitioned_epoch_rewards_compare_calculation")
                        .long("partitioned-epoch-rewards-compare-calculation")
                        .takes_value(false)
                        .help(
                            "Do normal epoch rewards distribution, but also calculate rewards \
                             using the partitioned rewards code path and compare the resulting \
                             vote and stake accounts",
                        )
                        .hidden(hidden_unless_forced()),
                )
                .arg(
                    Arg::with_name("partitioned_epoch_rewards_force_enable_single_slot")
                        .long("partitioned-epoch-rewards-force-enable-single-slot")
                        .takes_value(false)
                        .help(
                            "Force the partitioned rewards distribution, but distribute all \
                             rewards in the first slot in the epoch. This should match consensus \
                             with the normal rewards distribution.",
                        )
                        .conflicts_with("partitioned_epoch_rewards_compare_calculation")
                        .hidden(hidden_unless_forced()),
                )
                .arg(
                    Arg::with_name("print_accounts_stats")
                        .long("print-accounts-stats")
                        .takes_value(false)
                        .help(
                            "After verifying the ledger, print some information about the account \
                             stores",
                        ),
                )
                .arg(
                    Arg::with_name("write_bank_file")
                        .long("write-bank-file")
                        .takes_value(false)
                        .help(
                            "After verifying the ledger, write a file that contains the \
                             information that went into computing the completed bank's bank hash. \
                             The file will be written within <LEDGER_DIR>/bank_hash_details/",
                        ),
                )
                .arg(
                    Arg::with_name("record_slots")
                        .long("record-slots")
                        .default_value("slots.json")
                        .value_name("FILENAME")
                        .help("Record slots to a file"),
                )
                .arg(
                    Arg::with_name("verify_slots")
                        .long("verify-slots")
                        .default_value("slots.json")
                        .value_name("FILENAME")
                        .help("Verify slots match contents of file"),
                )
                .arg(
                    Arg::with_name("record_slots_config")
                        .long("record-slots-config")
                        .default_value("hash-only")
                        .possible_values(&["hash-only", "accounts"])
                        .requires("record_slots")
                        .help("In the slot recording, include bank details or not"),
                ),
        )
        .subcommand(
            SubCommand::with_name("graph")
                .about("Create a Graphviz rendering of the ledger")
                .arg(&no_snapshot_arg)
                .arg(&account_paths_arg)
                .arg(&accounts_hash_cache_path_arg)
                .arg(&accounts_index_bins)
                .arg(&accounts_index_limit)
                .arg(&disable_disk_index)
                .arg(&accountsdb_verify_refcounts)
                .arg(&accounts_db_test_skip_rewrites_but_include_in_bank_hash)
                .arg(&accounts_db_skip_initial_hash_calc_arg)
                .arg(&halt_at_slot_arg)
                .arg(&hard_forks_arg)
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(&use_snapshot_archives_at_startup)
                .arg(
                    Arg::with_name("include_all_votes")
                        .long("include-all-votes")
                        .help("Include all votes in the graph"),
                )
                .arg(
                    Arg::with_name("graph_filename")
                        .index(1)
                        .value_name("FILENAME")
                        .takes_value(true)
                        .help("Output file"),
                )
                .arg(
                    Arg::with_name("vote_account_mode")
                        .long("vote-account-mode")
                        .takes_value(true)
                        .value_name("MODE")
                        .default_value(default_graph_vote_account_mode.as_ref())
                        .possible_values(GraphVoteAccountMode::ALL_MODE_STRINGS)
                        .help(
                            "Specify if and how to graph vote accounts. Enabling will incur \
                             significant rendering overhead, especially `with-history`",
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-snapshot")
                .about("Create a new ledger snapshot")
                .arg(&no_snapshot_arg)
                .arg(&account_paths_arg)
                .arg(&accounts_hash_cache_path_arg)
                .arg(&accounts_index_bins)
                .arg(&accounts_index_limit)
                .arg(&disable_disk_index)
                .arg(&accountsdb_verify_refcounts)
                .arg(&accounts_db_test_skip_rewrites_but_include_in_bank_hash)
                .arg(&accounts_db_skip_initial_hash_calc_arg)
                .arg(&accountsdb_skip_shrink)
                .arg(&ancient_append_vecs)
                .arg(&hard_forks_arg)
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(&snapshot_version_arg)
                .arg(&maximum_full_snapshot_archives_to_retain)
                .arg(&maximum_incremental_snapshot_archives_to_retain)
                .arg(&geyser_plugin_args)
                .arg(&use_snapshot_archives_at_startup)
                .arg(
                    Arg::with_name("snapshot_slot")
                        .index(1)
                        .value_name("SLOT")
                        .validator(|value| {
                            if value.parse::<Slot>().is_ok() || value == "ROOT" {
                                Ok(())
                            } else {
                                Err(format!(
                                    "Unable to parse as a number or the keyword ROOT, provided: \
                                     {value}"
                                ))
                            }
                        })
                        .takes_value(true)
                        .help(
                            "Slot at which to create the snapshot; accepts keyword ROOT for the \
                             highest root",
                        ),
                )
                .arg(
                    Arg::with_name("output_directory")
                        .index(2)
                        .value_name("DIR")
                        .takes_value(true)
                        .help(
                            "Output directory for the snapshot \
                            [default: --snapshot-archive-path if present else --ledger directory]",
                        ),
                )
                .arg(
                    Arg::with_name("warp_slot")
                        .required(false)
                        .long("warp-slot")
                        .takes_value(true)
                        .value_name("WARP_SLOT")
                        .validator(is_slot)
                        .help(
                            "After loading the snapshot slot warp the ledger to WARP_SLOT, which \
                             could be a slot in a galaxy far far away",
                        ),
                )
                .arg(
                    Arg::with_name("faucet_lamports")
                        .short("t")
                        .long("faucet-lamports")
                        .value_name("LAMPORTS")
                        .takes_value(true)
                        .requires("faucet_pubkey")
                        .help("Number of lamports to assign to the faucet"),
                )
                .arg(
                    Arg::with_name("faucet_pubkey")
                        .short("m")
                        .long("faucet-pubkey")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .requires("faucet_lamports")
                        .help("Path to file containing the faucet's pubkey"),
                )
                .arg(
                    Arg::with_name("bootstrap_validator")
                        .short("b")
                        .long("bootstrap-validator")
                        .value_name("IDENTITY_PUBKEY VOTE_PUBKEY STAKE_PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .number_of_values(3)
                        .multiple(true)
                        .help("The bootstrap validator's identity, vote and stake pubkeys"),
                )
                .arg(
                    Arg::with_name("bootstrap_stake_authorized_pubkey")
                        .long("bootstrap-stake-authorized-pubkey")
                        .value_name("BOOTSTRAP STAKE AUTHORIZED PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help(
                            "Path to file containing the pubkey authorized to manage the \
                             bootstrap validator's stake
                             [default: --bootstrap-validator IDENTITY_PUBKEY]",
                        ),
                )
                .arg(
                    Arg::with_name("bootstrap_validator_lamports")
                        .long("bootstrap-validator-lamports")
                        .value_name("LAMPORTS")
                        .takes_value(true)
                        .default_value(default_bootstrap_validator_lamports)
                        .help("Number of lamports to assign to the bootstrap validator"),
                )
                .arg(
                    Arg::with_name("bootstrap_validator_stake_lamports")
                        .long("bootstrap-validator-stake-lamports")
                        .value_name("LAMPORTS")
                        .takes_value(true)
                        .default_value(default_bootstrap_validator_stake_lamports)
                        .help(
                            "Number of lamports to assign to the bootstrap validator's stake \
                             account",
                        ),
                )
                .arg(
                    Arg::with_name("rent_burn_percentage")
                        .long("rent-burn-percentage")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .help("Adjust percentage of collected rent to burn")
                        .validator(is_valid_percentage),
                )
                .arg(&hashes_per_tick)
                .arg(
                    Arg::with_name("accounts_to_remove")
                        .required(false)
                        .long("remove-account")
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_pubkey)
                        .multiple(true)
                        .help("List of accounts to remove while creating the snapshot"),
                )
                .arg(
                    Arg::with_name("feature_gates_to_deactivate")
                        .required(false)
                        .long("deactivate-feature-gate")
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_pubkey)
                        .multiple(true)
                        .help("List of feature gates to deactivate while creating the snapshot"),
                )
                .arg(
                    Arg::with_name("vote_accounts_to_destake")
                        .required(false)
                        .long("destake-vote-account")
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_pubkey)
                        .multiple(true)
                        .help("List of validator vote accounts to destake"),
                )
                .arg(
                    Arg::with_name("remove_stake_accounts")
                        .required(false)
                        .long("remove-stake-accounts")
                        .takes_value(false)
                        .help("Remove all existing stake accounts from the new snapshot"),
                )
                .arg(
                    Arg::with_name("incremental")
                        .long("incremental")
                        .takes_value(false)
                        .help(
                            "Create an incremental snapshot instead of a full snapshot. This \
                             requires that the ledger is loaded from a full snapshot, which will \
                             be used as the base for the incremental snapshot.",
                        )
                        .conflicts_with("no_snapshot"),
                )
                .arg(
                    Arg::with_name("minimized")
                        .long("minimized")
                        .takes_value(false)
                        .help(
                            "Create a minimized snapshot instead of a full snapshot. This \
                             snapshot will only include information needed to replay the ledger \
                             from the snapshot slot to the ending slot.",
                        )
                        .conflicts_with("incremental")
                        .requires("ending_slot"),
                )
                .arg(
                    Arg::with_name("ending_slot")
                        .long("ending-slot")
                        .takes_value(true)
                        .value_name("ENDING_SLOT")
                        .help("Ending slot for minimized snapshot creation"),
                )
                .arg(
                    Arg::with_name("snapshot_archive_format")
                        .long("snapshot-archive-format")
                        .possible_values(SUPPORTED_ARCHIVE_COMPRESSION)
                        .default_value(DEFAULT_ARCHIVE_COMPRESSION)
                        .value_name("ARCHIVE_TYPE")
                        .takes_value(true)
                        .help("Snapshot archive format to use.")
                        .conflicts_with("no_snapshot"),
                )
                .arg(
                    Arg::with_name("enable_capitalization_change")
                        .long("enable-capitalization-change")
                        .takes_value(false)
                        .help("If snapshot creation should succeed with a capitalization delta."),
                ),
        )
        .subcommand(
            SubCommand::with_name("accounts")
                .about("Print account stats and contents after processing the ledger")
                .arg(&no_snapshot_arg)
                .arg(&account_paths_arg)
                .arg(&accounts_hash_cache_path_arg)
                .arg(&accounts_index_bins)
                .arg(&accounts_index_limit)
                .arg(&disable_disk_index)
                .arg(&accountsdb_verify_refcounts)
                .arg(&accounts_db_test_skip_rewrites_but_include_in_bank_hash)
                .arg(&accounts_db_skip_initial_hash_calc_arg)
                .arg(&halt_at_slot_arg)
                .arg(&hard_forks_arg)
                .arg(&geyser_plugin_args)
                .arg(&accounts_data_encoding_arg)
                .arg(&use_snapshot_archives_at_startup)
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(
                    Arg::with_name("include_sysvars")
                        .long("include-sysvars")
                        .takes_value(false)
                        .help("Include sysvars too"),
                )
                .arg(
                    Arg::with_name("no_account_contents")
                        .long("no-account-contents")
                        .takes_value(false)
                        .help(
                            "Do not print contents of each account, which is very slow with lots \
                             of accounts.",
                        ),
                )
                .arg(
                    Arg::with_name("no_account_data")
                        .long("no-account-data")
                        .takes_value(false)
                        .help("Do not print account data when printing account contents."),
                )
                .arg(
                    Arg::with_name("account")
                        .long("account")
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_pubkey)
                        .multiple(true)
                        .help(
                            "Limit output to accounts corresponding to the specified pubkey(s), \
                            may be specified multiple times",
                        ),
                )
                .arg(
                    Arg::with_name("program_accounts")
                        .long("program-accounts")
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_pubkey)
                        .conflicts_with("account")
                        .help("Limit output to accounts owned by the provided program pubkey"),
                ),
        )
        .subcommand(
            SubCommand::with_name("capitalization")
                .about("Print capitalization (aka, total supply) while checksumming it")
                .arg(&no_snapshot_arg)
                .arg(&account_paths_arg)
                .arg(&accounts_hash_cache_path_arg)
                .arg(&accounts_index_bins)
                .arg(&accounts_index_limit)
                .arg(&disable_disk_index)
                .arg(&accountsdb_verify_refcounts)
                .arg(&accounts_db_test_skip_rewrites_but_include_in_bank_hash)
                .arg(&accounts_db_skip_initial_hash_calc_arg)
                .arg(&halt_at_slot_arg)
                .arg(&hard_forks_arg)
                .arg(&max_genesis_archive_unpacked_size_arg)
                .arg(&geyser_plugin_args)
                .arg(&use_snapshot_archives_at_startup)
                .arg(
                    Arg::with_name("warp_epoch")
                        .required(false)
                        .long("warp-epoch")
                        .takes_value(true)
                        .value_name("WARP_EPOCH")
                        .help(
                            "After loading the snapshot warp the ledger to WARP_EPOCH, which \
                             could be an epoch in a galaxy far far away",
                        ),
                )
                .arg(
                    Arg::with_name("inflation")
                        .required(false)
                        .long("inflation")
                        .takes_value(true)
                        .possible_values(&["pico", "full", "none"])
                        .help("Overwrite inflation when warping"),
                )
                .arg(
                    Arg::with_name("enable_credits_auto_rewind")
                        .required(false)
                        .long("enable-credits-auto-rewind")
                        .takes_value(false)
                        .help("Enable credits auto rewind"),
                )
                .arg(
                    Arg::with_name("recalculate_capitalization")
                        .required(false)
                        .long("recalculate-capitalization")
                        .takes_value(false)
                        .help(
                            "Recalculate capitalization before warping; circumvents bank's \
                             out-of-sync capitalization",
                        ),
                )
                .arg(
                    Arg::with_name("csv_filename")
                        .long("csv-filename")
                        .value_name("FILENAME")
                        .takes_value(true)
                        .help("Output file in the csv format"),
                ),
        )
        .subcommand(
            SubCommand::with_name("compute-slot-cost")
                .about(
                    "runs cost_model over the block at the given slots, computes how expensive a \
                     block was based on cost_model",
                )
                .arg(
                    Arg::with_name("slots")
                        .index(1)
                        .value_name("SLOTS")
                        .validator(is_slot)
                        .multiple(true)
                        .takes_value(true)
                        .help(
                            "Slots that their blocks are computed for cost, default to all slots \
                             in ledger",
                        ),
                )
                .arg(&allow_dead_slots_arg),
        )
        .program_subcommand()
        .get_matches();

    info!("{} {}", crate_name!(), solana_version::version!());

    let ledger_path = PathBuf::from(value_t_or_exit!(matches, "ledger_path", String));
    let snapshot_archive_path = value_t!(matches, "snapshots", String)
        .ok()
        .map(PathBuf::from);
    let incremental_snapshot_archive_path =
        value_t!(matches, "incremental_snapshot_archive_path", String)
            .ok()
            .map(PathBuf::from);

    let verbose_level = matches.occurrences_of("verbose");

    match matches.subcommand() {
        ("bigtable", Some(arg_matches)) => bigtable_process_command(&ledger_path, arg_matches),
        ("blockstore", Some(arg_matches)) => blockstore_process_command(&ledger_path, arg_matches),
        ("program", Some(arg_matches)) => program(&ledger_path, arg_matches),
        // This match case provides legacy support for commands that were previously top level
        // subcommands of the binary, but have been moved under the blockstore subcommand.
        ("analyze-storage", Some(_))
        | ("bounds", Some(_))
        | ("copy", Some(_))
        | ("dead-slots", Some(_))
        | ("duplicate-slots", Some(_))
        | ("json", Some(_))
        | ("latest-optimistic-slots", Some(_))
        | ("list-roots", Some(_))
        | ("parse_full_frozen", Some(_))
        | ("print", Some(_))
        | ("print-file-metadata", Some(_))
        | ("purge", Some(_))
        | ("remove-dead-slot", Some(_))
        | ("repair-roots", Some(_))
        | ("set-dead-slot", Some(_))
        | ("shred-meta", Some(_))
        | ("slot", Some(_)) => blockstore_process_command(&ledger_path, &matches),
        _ => {
            let ledger_path = canonicalize_ledger_path(&ledger_path);

            match matches.subcommand() {
                ("genesis", Some(arg_matches)) => {
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let print_accounts = arg_matches.is_present("accounts");
                    if print_accounts {
                        let print_account_data = !arg_matches.is_present("no_account_data");
                        let print_encoding_format = parse_encoding_format(arg_matches);
                        for (pubkey, account) in genesis_config.accounts {
                            output_account(
                                &pubkey,
                                &AccountSharedData::from(account),
                                None,
                                print_account_data,
                                print_encoding_format,
                            );
                        }
                    } else {
                        println!("{genesis_config}");
                    }
                }
                ("genesis-hash", Some(arg_matches)) => {
                    println!(
                        "{}",
                        open_genesis_config_by(&ledger_path, arg_matches).hash()
                    );
                }
                ("modify-genesis", Some(arg_matches)) => {
                    let mut genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let output_directory =
                        PathBuf::from(arg_matches.value_of("output_directory").unwrap());

                    if let Some(cluster_type) = cluster_type_of(arg_matches, "cluster_type") {
                        genesis_config.cluster_type = cluster_type;
                    }

                    if let Some(hashes_per_tick) = arg_matches.value_of("hashes_per_tick") {
                        genesis_config.poh_config.hashes_per_tick = match hashes_per_tick {
                            // Note: Unlike `solana-genesis`, "auto" is not supported here.
                            "sleep" => None,
                            _ => Some(value_t_or_exit!(arg_matches, "hashes_per_tick", u64)),
                        }
                    }

                    create_new_ledger(
                        &output_directory,
                        &genesis_config,
                        solana_accounts_db::hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
                        LedgerColumnOptions::default(),
                    )
                    .unwrap_or_else(|err| {
                        eprintln!("Failed to write genesis config: {err:?}");
                        exit(1);
                    });

                    println!("{}", open_genesis_config_by(&output_directory, arg_matches));
                }
                ("shred-version", Some(arg_matches)) => {
                    let mut process_options = parse_process_options(&ledger_path, arg_matches);
                    // Respect a user-set --halt-at-slot; otherwise, set Some(0) to avoid
                    // processing any additional banks and just use the snapshot bank
                    if process_options.halt_at_slot.is_none() {
                        process_options.halt_at_slot = Some(0);
                    }
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let blockstore = open_blockstore(
                        &ledger_path,
                        arg_matches,
                        get_access_type(&process_options),
                    );
                    let (bank_forks, _) = load_and_process_ledger_or_exit(
                        arg_matches,
                        &genesis_config,
                        Arc::new(blockstore),
                        process_options,
                        snapshot_archive_path,
                        incremental_snapshot_archive_path,
                    );

                    println!(
                        "{}",
                        compute_shred_version(
                            &genesis_config.hash(),
                            Some(&bank_forks.read().unwrap().working_bank().hard_forks())
                        )
                    );
                }
                ("bank-hash", Some(arg_matches)) => {
                    let process_options = parse_process_options(&ledger_path, arg_matches);
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let blockstore = open_blockstore(
                        &ledger_path,
                        arg_matches,
                        get_access_type(&process_options),
                    );
                    let (bank_forks, _) = load_and_process_ledger_or_exit(
                        arg_matches,
                        &genesis_config,
                        Arc::new(blockstore),
                        process_options,
                        snapshot_archive_path,
                        incremental_snapshot_archive_path,
                    );
                    println!("{}", &bank_forks.read().unwrap().working_bank().hash());
                }
                ("verify", Some(arg_matches)) => {
                    let exit_signal = Arc::new(AtomicBool::new(false));
                    let report_os_memory_stats =
                        arg_matches.is_present("os_memory_stats_reporting");
                    let system_monitor_service = SystemMonitorService::new(
                        Arc::clone(&exit_signal),
                        SystemMonitorStatsReportConfig {
                            report_os_memory_stats,
                            report_os_network_stats: false,
                            report_os_cpu_stats: false,
                            report_os_disk_stats: false,
                        },
                    );

                    let mut process_options = parse_process_options(&ledger_path, arg_matches);

                    // .default_value() does not work with .conflicts_with() in clap 2.33
                    // .conflicts_with("verify_slots")
                    // https://github.com/clap-rs/clap/issues/1605#issuecomment-722326915
                    // So open-code the conflicts_with() here
                    if arg_matches.occurrences_of("record_slots") > 0
                        && arg_matches.occurrences_of("verify_slots") > 0
                    {
                        eprintln!(
                            "error: The argument '--verify-slots <FILENAME>' cannot be used with '--record-slots <FILENAME>'"
                        );
                        exit(1);
                    }

                    let (slot_callback, record_slots_file, recorded_slots) = if arg_matches
                        .occurrences_of("record_slots")
                        > 0
                    {
                        let filename = Path::new(arg_matches.value_of_os("record_slots").unwrap());

                        let file = File::create(filename).unwrap_or_else(|err| {
                            eprintln!("Unable to write to file: {}: {:#}", filename.display(), err);
                            exit(1);
                        });

                        let include_bank =
                            match arg_matches.value_of("record_slots_config").unwrap() {
                                "hash-only" => false,
                                "accounts" => true,
                                _ => unreachable!(),
                            };

                        let slot_hashes = Arc::new(Mutex::new(Vec::new()));

                        let slot_callback = Arc::new({
                            let slots = Arc::clone(&slot_hashes);
                            move |bank: &Bank| {
                                let slot_details = if include_bank {
                                    bank_hash_details::BankHashSlotDetails::try_from(bank).unwrap()
                                } else {
                                    bank_hash_details::BankHashSlotDetails {
                                        slot: bank.slot(),
                                        bank_hash: bank.hash().to_string(),
                                        ..Default::default()
                                    }
                                };

                                slots.lock().unwrap().push(slot_details);
                            }
                        });

                        (
                            Some(slot_callback as ProcessSlotCallback),
                            Some(file),
                            Some(slot_hashes),
                        )
                    } else if arg_matches.occurrences_of("verify_slots") > 0 {
                        let filename = Path::new(arg_matches.value_of_os("verify_slots").unwrap());

                        let file = File::open(filename).unwrap_or_else(|err| {
                            eprintln!("Unable to read file: {}: {err:#}", filename.display());
                            exit(1);
                        });

                        let reader = std::io::BufReader::new(file);

                        let details: bank_hash_details::BankHashDetails =
                            serde_json::from_reader(reader).unwrap_or_else(|err| {
                                eprintln!("Error loading slots file: {err:#}");
                                exit(1);
                            });

                        let slots = Arc::new(Mutex::new(details.bank_hash_details));

                        let slot_callback = Arc::new(move |bank: &Bank| {
                            if slots.lock().unwrap().is_empty() {
                                error!(
                                    "Expected slot: not found got slot: {} hash: {}",
                                    bank.slot(),
                                    bank.hash()
                                );
                            } else {
                                let bank_hash_details::BankHashSlotDetails {
                                    slot: expected_slot,
                                    bank_hash: expected_hash,
                                    ..
                                } = slots.lock().unwrap().remove(0);
                                if bank.slot() != expected_slot
                                    || bank.hash().to_string() != expected_hash
                                {
                                    error!("Expected slot: {expected_slot} hash: {expected_hash} got slot: {} hash: {}",
                                bank.slot(), bank.hash());
                                } else {
                                    info!(
                                    "Expected slot: {expected_slot} hash: {expected_hash} correct"
                                );
                                }
                            }
                        });

                        (Some(slot_callback as ProcessSlotCallback), None, None)
                    } else {
                        (None, None, None)
                    };

                    process_options.slot_callback = slot_callback;

                    let print_accounts_stats = arg_matches.is_present("print_accounts_stats");
                    let write_bank_file = arg_matches.is_present("write_bank_file");
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    info!("genesis hash: {}", genesis_config.hash());

                    let blockstore = open_blockstore(
                        &ledger_path,
                        arg_matches,
                        get_access_type(&process_options),
                    );
                    let (bank_forks, _) = load_and_process_ledger_or_exit(
                        arg_matches,
                        &genesis_config,
                        Arc::new(blockstore),
                        process_options,
                        snapshot_archive_path,
                        incremental_snapshot_archive_path,
                    );

                    if print_accounts_stats {
                        let working_bank = bank_forks.read().unwrap().working_bank();
                        working_bank.print_accounts_stats();
                    }
                    if write_bank_file {
                        let working_bank = bank_forks.read().unwrap().working_bank();
                        bank_hash_details::write_bank_hash_details_file(&working_bank)
                            .map_err(|err| {
                                warn!("Unable to write bank hash_details file: {err}");
                            })
                            .ok();
                    }

                    if let Some(recorded_slots_file) = record_slots_file {
                        if let Ok(recorded_slots) = recorded_slots.clone().unwrap().lock() {
                            let bank_hashes =
                                bank_hash_details::BankHashDetails::new(recorded_slots.to_vec());

                            // writing the json file ends up with a syscall for each number, comma, indentation etc.
                            // use BufWriter to speed things up

                            let writer = std::io::BufWriter::new(recorded_slots_file);

                            serde_json::to_writer_pretty(writer, &bank_hashes).unwrap();
                        }
                    }

                    exit_signal.store(true, Ordering::Relaxed);
                    system_monitor_service.join().unwrap();
                }
                ("graph", Some(arg_matches)) => {
                    let output_file = value_t_or_exit!(arg_matches, "graph_filename", String);
                    let graph_config = GraphConfig {
                        include_all_votes: arg_matches.is_present("include_all_votes"),
                        vote_account_mode: value_t_or_exit!(
                            arg_matches,
                            "vote_account_mode",
                            GraphVoteAccountMode
                        ),
                    };

                    let process_options = parse_process_options(&ledger_path, arg_matches);
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let blockstore = open_blockstore(
                        &ledger_path,
                        arg_matches,
                        get_access_type(&process_options),
                    );
                    let (bank_forks, _) = load_and_process_ledger_or_exit(
                        arg_matches,
                        &genesis_config,
                        Arc::new(blockstore),
                        process_options,
                        snapshot_archive_path,
                        incremental_snapshot_archive_path,
                    );

                    let dot = graph_forks(&bank_forks.read().unwrap(), &graph_config);
                    let extension = Path::new(&output_file).extension();
                    let result = if extension == Some(OsStr::new("pdf")) {
                        render_dot(dot, &output_file, "pdf")
                    } else if extension == Some(OsStr::new("png")) {
                        render_dot(dot, &output_file, "png")
                    } else {
                        File::create(&output_file)
                            .and_then(|mut file| file.write_all(&dot.into_bytes()))
                    };

                    match result {
                        Ok(_) => println!("Wrote {output_file}"),
                        Err(err) => eprintln!("Unable to write {output_file}: {err}"),
                    }
                }
                ("create-snapshot", Some(arg_matches)) => {
                    let is_incremental = arg_matches.is_present("incremental");
                    let is_minimized = arg_matches.is_present("minimized");
                    let output_directory = value_t!(arg_matches, "output_directory", PathBuf)
                        .unwrap_or_else(|_| {
                            match (
                                is_incremental,
                                &snapshot_archive_path,
                                &incremental_snapshot_archive_path,
                            ) {
                                (true, _, Some(incremental_snapshot_archive_path)) => {
                                    incremental_snapshot_archive_path.clone()
                                }
                                (_, Some(snapshot_archive_path), _) => {
                                    snapshot_archive_path.clone()
                                }
                                (_, _, _) => ledger_path.clone(),
                            }
                        });
                    let mut warp_slot = value_t!(arg_matches, "warp_slot", Slot).ok();
                    let remove_stake_accounts = arg_matches.is_present("remove_stake_accounts");

                    let faucet_pubkey = pubkey_of(arg_matches, "faucet_pubkey");
                    let faucet_lamports =
                        value_t!(arg_matches, "faucet_lamports", u64).unwrap_or(0);

                    let rent_burn_percentage = value_t!(arg_matches, "rent_burn_percentage", u8);
                    let hashes_per_tick = arg_matches.value_of("hashes_per_tick");

                    let bootstrap_stake_authorized_pubkey =
                        pubkey_of(arg_matches, "bootstrap_stake_authorized_pubkey");
                    let bootstrap_validator_lamports =
                        value_t_or_exit!(arg_matches, "bootstrap_validator_lamports", u64);
                    let bootstrap_validator_stake_lamports =
                        value_t_or_exit!(arg_matches, "bootstrap_validator_stake_lamports", u64);
                    let minimum_stake_lamports = rent.minimum_balance(StakeStateV2::size_of());
                    if bootstrap_validator_stake_lamports < minimum_stake_lamports {
                        eprintln!(
                        "Error: insufficient --bootstrap-validator-stake-lamports. Minimum amount \
                         is {minimum_stake_lamports}"
                    );
                        exit(1);
                    }
                    let bootstrap_validator_pubkeys =
                        pubkeys_of(arg_matches, "bootstrap_validator");
                    let accounts_to_remove =
                        pubkeys_of(arg_matches, "accounts_to_remove").unwrap_or_default();
                    let feature_gates_to_deactivate =
                        pubkeys_of(arg_matches, "feature_gates_to_deactivate").unwrap_or_default();
                    let vote_accounts_to_destake: HashSet<_> =
                        pubkeys_of(arg_matches, "vote_accounts_to_destake")
                            .unwrap_or_default()
                            .into_iter()
                            .collect();
                    let snapshot_version = arg_matches.value_of("snapshot_version").map_or(
                        SnapshotVersion::default(),
                        |s| {
                            s.parse::<SnapshotVersion>().unwrap_or_else(|e| {
                                eprintln!("Error: {e}");
                                exit(1)
                            })
                        },
                    );

                    let snapshot_archive_format = {
                        let archive_format_str =
                            value_t_or_exit!(arg_matches, "snapshot_archive_format", String);
                        ArchiveFormat::from_cli_arg(&archive_format_str).unwrap_or_else(|| {
                            panic!("Archive format not recognized: {archive_format_str}")
                        })
                    };

                    let maximum_full_snapshot_archives_to_retain = value_t_or_exit!(
                        arg_matches,
                        "maximum_full_snapshots_to_retain",
                        NonZeroUsize
                    );
                    let maximum_incremental_snapshot_archives_to_retain = value_t_or_exit!(
                        arg_matches,
                        "maximum_incremental_snapshots_to_retain",
                        NonZeroUsize
                    );
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let mut process_options = parse_process_options(&ledger_path, arg_matches);

                    let blockstore = Arc::new(open_blockstore(
                        &ledger_path,
                        arg_matches,
                        get_access_type(&process_options),
                    ));

                    let snapshot_slot = if Some("ROOT") == arg_matches.value_of("snapshot_slot") {
                        blockstore
                            .rooted_slot_iterator(0)
                            .expect("Failed to get rooted slot iterator")
                            .last()
                            .expect("Failed to get root")
                    } else {
                        value_t_or_exit!(arg_matches, "snapshot_slot", Slot)
                    };

                    if blockstore
                        .meta(snapshot_slot)
                        .unwrap()
                        .filter(|m| m.is_full())
                        .is_none()
                    {
                        eprintln!(
                        "Error: snapshot slot {snapshot_slot} does not exist in blockstore or is \
                         not full.",
                    );
                        exit(1);
                    }
                    process_options.halt_at_slot = Some(snapshot_slot);

                    let ending_slot = if is_minimized {
                        let ending_slot = value_t_or_exit!(arg_matches, "ending_slot", Slot);
                        if ending_slot <= snapshot_slot {
                            eprintln!(
                                "Error: ending_slot ({ending_slot}) must be greater than \
                             snapshot_slot ({snapshot_slot})"
                            );
                            exit(1);
                        }

                        Some(ending_slot)
                    } else {
                        None
                    };

                    let enable_capitalization_change =
                        arg_matches.is_present("enable_capitalization_change");

                    let snapshot_type_str = if is_incremental {
                        "incremental "
                    } else if is_minimized {
                        "minimized "
                    } else {
                        ""
                    };

                    info!(
                        "Creating {}snapshot of slot {} in {}",
                        snapshot_type_str,
                        snapshot_slot,
                        output_directory.display()
                    );

                    let (bank_forks, starting_snapshot_hashes) = load_and_process_ledger_or_exit(
                        arg_matches,
                        &genesis_config,
                        blockstore.clone(),
                        process_options,
                        snapshot_archive_path,
                        incremental_snapshot_archive_path,
                    );
                    let mut bank = bank_forks
                        .read()
                        .unwrap()
                        .get(snapshot_slot)
                        .unwrap_or_else(|| {
                            eprintln!("Error: Slot {snapshot_slot} is not available");
                            exit(1);
                        });

                    let child_bank_required = rent_burn_percentage.is_ok()
                        || hashes_per_tick.is_some()
                        || remove_stake_accounts
                        || !accounts_to_remove.is_empty()
                        || !feature_gates_to_deactivate.is_empty()
                        || !vote_accounts_to_destake.is_empty()
                        || faucet_pubkey.is_some()
                        || bootstrap_validator_pubkeys.is_some();

                    if child_bank_required {
                        let mut child_bank = Bank::new_from_parent(
                            bank.clone(),
                            bank.collector_id(),
                            bank.slot() + 1,
                        );

                        if let Ok(rent_burn_percentage) = rent_burn_percentage {
                            child_bank.set_rent_burn_percentage(rent_burn_percentage);
                        }

                        if let Some(hashes_per_tick) = hashes_per_tick {
                            child_bank.set_hashes_per_tick(match hashes_per_tick {
                                // Note: Unlike `solana-genesis`, "auto" is not supported here.
                                "sleep" => None,
                                _ => Some(value_t_or_exit!(arg_matches, "hashes_per_tick", u64)),
                            });
                        }
                        bank = Arc::new(child_bank);
                    }

                    if let Some(faucet_pubkey) = faucet_pubkey {
                        bank.store_account(
                            &faucet_pubkey,
                            &AccountSharedData::new(faucet_lamports, 0, &system_program::id()),
                        );
                    }

                    if remove_stake_accounts {
                        for (address, mut account) in bank
                            .get_program_accounts(&stake::program::id(), &ScanConfig::default())
                            .unwrap()
                            .into_iter()
                        {
                            account.set_lamports(0);
                            bank.store_account(&address, &account);
                        }
                    }

                    for address in accounts_to_remove {
                        let mut account = bank.get_account(&address).unwrap_or_else(|| {
                            eprintln!(
                                "Error: Account does not exist, unable to remove it: {address}"
                            );
                            exit(1);
                        });

                        account.set_lamports(0);
                        bank.store_account(&address, &account);
                        debug!("Account removed: {address}");
                    }

                    for address in feature_gates_to_deactivate {
                        let mut account = bank.get_account(&address).unwrap_or_else(|| {
                            eprintln!(
                                "Error: Feature-gate account does not exist, unable to \
                                     deactivate it: {address}"
                            );
                            exit(1);
                        });

                        match feature::from_account(&account) {
                            Some(feature) => {
                                if feature.activated_at.is_none() {
                                    warn!("Feature gate is not yet activated: {address}");
                                }
                            }
                            None => {
                                eprintln!("Error: Account is not a `Feature`: {address}");
                                exit(1);
                            }
                        }

                        account.set_lamports(0);
                        bank.store_account(&address, &account);
                        debug!("Feature gate deactivated: {address}");
                    }

                    if !vote_accounts_to_destake.is_empty() {
                        for (address, mut account) in bank
                            .get_program_accounts(&stake::program::id(), &ScanConfig::default())
                            .unwrap()
                            .into_iter()
                        {
                            if let Ok(StakeStateV2::Stake(meta, stake, _)) = account.state() {
                                if vote_accounts_to_destake.contains(&stake.delegation.voter_pubkey)
                                {
                                    if verbose_level > 0 {
                                        warn!(
                                            "Undelegating stake account {} from {}",
                                            address, stake.delegation.voter_pubkey,
                                        );
                                    }
                                    account.set_state(&StakeStateV2::Initialized(meta)).unwrap();
                                    bank.store_account(&address, &account);
                                }
                            }
                        }
                    }

                    if let Some(bootstrap_validator_pubkeys) = bootstrap_validator_pubkeys {
                        assert_eq!(bootstrap_validator_pubkeys.len() % 3, 0);

                        // Ensure there are no duplicated pubkeys in the --bootstrap-validator list
                        {
                            let mut v = bootstrap_validator_pubkeys.clone();
                            v.sort();
                            v.dedup();
                            if v.len() != bootstrap_validator_pubkeys.len() {
                                eprintln!(
                                    "Error: --bootstrap-validator pubkeys cannot be duplicated"
                                );
                                exit(1);
                            }
                        }

                        // Delete existing vote accounts
                        for (address, mut account) in bank
                            .get_program_accounts(
                                &solana_vote_program::id(),
                                &ScanConfig::default(),
                            )
                            .unwrap()
                            .into_iter()
                        {
                            account.set_lamports(0);
                            bank.store_account(&address, &account);
                        }

                        // Add a new identity/vote/stake account for each of the provided bootstrap
                        // validators
                        let mut bootstrap_validator_pubkeys_iter =
                            bootstrap_validator_pubkeys.iter();
                        loop {
                            let Some(identity_pubkey) = bootstrap_validator_pubkeys_iter.next()
                            else {
                                break;
                            };
                            let vote_pubkey = bootstrap_validator_pubkeys_iter.next().unwrap();
                            let stake_pubkey = bootstrap_validator_pubkeys_iter.next().unwrap();

                            bank.store_account(
                                identity_pubkey,
                                &AccountSharedData::new(
                                    bootstrap_validator_lamports,
                                    0,
                                    &system_program::id(),
                                ),
                            );

                            let vote_account = vote_state::create_account_with_authorized(
                                identity_pubkey,
                                identity_pubkey,
                                identity_pubkey,
                                100,
                                VoteState::get_rent_exempt_reserve(&rent).max(1),
                            );

                            bank.store_account(
                                stake_pubkey,
                                &stake_state::create_account(
                                    bootstrap_stake_authorized_pubkey
                                        .as_ref()
                                        .unwrap_or(identity_pubkey),
                                    vote_pubkey,
                                    &vote_account,
                                    &rent,
                                    bootstrap_validator_stake_lamports,
                                ),
                            );
                            bank.store_account(vote_pubkey, &vote_account);
                        }

                        // Warp ahead at least two epochs to ensure that the leader schedule will be
                        // updated to reflect the new bootstrap validator(s)
                        let minimum_warp_slot =
                            genesis_config.epoch_schedule.get_first_slot_in_epoch(
                                genesis_config.epoch_schedule.get_epoch(snapshot_slot) + 2,
                            );

                        if let Some(warp_slot) = warp_slot {
                            if warp_slot < minimum_warp_slot {
                                eprintln!(
                                    "Error: --warp-slot too close.  Must be >= \
                                         {minimum_warp_slot}"
                                );
                                exit(1);
                            }
                        } else {
                            warn!("Warping to slot {}", minimum_warp_slot);
                            warp_slot = Some(minimum_warp_slot);
                        }
                    }

                    if child_bank_required {
                        while !bank.is_complete() {
                            bank.register_unique_tick();
                        }
                    }

                    let pre_capitalization = bank.capitalization();

                    bank.set_capitalization();

                    let post_capitalization = bank.capitalization();

                    let capitalization_message = if pre_capitalization != post_capitalization {
                        let amount = if pre_capitalization > post_capitalization {
                            format!("-{}", pre_capitalization - post_capitalization)
                        } else {
                            (post_capitalization - pre_capitalization).to_string()
                        };
                        let msg = format!("Capitalization change: {amount} lamports");
                        warn!("{msg}");
                        if !enable_capitalization_change {
                            eprintln!(
                                "{msg}\nBut `--enable-capitalization-change flag not provided"
                            );
                            exit(1);
                        }
                        Some(msg)
                    } else {
                        None
                    };

                    let bank = if let Some(warp_slot) = warp_slot {
                        // need to flush the write cache in order to use Storages to calculate
                        // the accounts hash, and need to root `bank` before flushing the cache
                        bank.rc.accounts.accounts_db.add_root(bank.slot());
                        bank.force_flush_accounts_cache();
                        Arc::new(Bank::warp_from_parent(
                            bank.clone(),
                            bank.collector_id(),
                            warp_slot,
                            CalcAccountsHashDataSource::Storages,
                        ))
                    } else {
                        bank
                    };

                    let minimize_snapshot_possibly_incomplete = if is_minimized {
                        minimize_bank_for_snapshot(
                            &blockstore,
                            &bank,
                            snapshot_slot,
                            ending_slot.unwrap(),
                        )
                    } else {
                        false
                    };

                    println!(
                        "Creating a version {} {}snapshot of slot {}",
                        snapshot_version,
                        snapshot_type_str,
                        bank.slot(),
                    );

                    if is_incremental {
                        if starting_snapshot_hashes.is_none() {
                            eprintln!(
                                "Unable to create incremental snapshot without a base full \
                                     snapshot"
                            );
                            exit(1);
                        }
                        let full_snapshot_slot = starting_snapshot_hashes.unwrap().full.0 .0;
                        if bank.slot() <= full_snapshot_slot {
                            eprintln!(
                                "Unable to create incremental snapshot: Slot must be greater \
                                     than full snapshot slot. slot: {}, full snapshot slot: {}",
                                bank.slot(),
                                full_snapshot_slot,
                            );
                            exit(1);
                        }

                        let incremental_snapshot_archive_info =
                            snapshot_bank_utils::bank_to_incremental_snapshot_archive(
                                ledger_path,
                                &bank,
                                full_snapshot_slot,
                                Some(snapshot_version),
                                output_directory.clone(),
                                output_directory,
                                snapshot_archive_format,
                                maximum_full_snapshot_archives_to_retain,
                                maximum_incremental_snapshot_archives_to_retain,
                            )
                            .unwrap_or_else(|err| {
                                eprintln!("Unable to create incremental snapshot: {err}");
                                exit(1);
                            });

                        println!(
                            "Successfully created incremental snapshot for slot {}, hash {}, \
                                 base slot: {}: {}",
                            bank.slot(),
                            bank.hash(),
                            full_snapshot_slot,
                            incremental_snapshot_archive_info.path().display(),
                        );
                    } else {
                        let full_snapshot_archive_info =
                            snapshot_bank_utils::bank_to_full_snapshot_archive(
                                ledger_path,
                                &bank,
                                Some(snapshot_version),
                                output_directory.clone(),
                                output_directory,
                                snapshot_archive_format,
                                maximum_full_snapshot_archives_to_retain,
                                maximum_incremental_snapshot_archives_to_retain,
                            )
                            .unwrap_or_else(|err| {
                                eprintln!("Unable to create snapshot: {err}");
                                exit(1);
                            });

                        println!(
                            "Successfully created snapshot for slot {}, hash {}: {}",
                            bank.slot(),
                            bank.hash(),
                            full_snapshot_archive_info.path().display(),
                        );

                        if is_minimized {
                            let starting_epoch = bank.epoch_schedule().get_epoch(snapshot_slot);
                            let ending_epoch =
                                bank.epoch_schedule().get_epoch(ending_slot.unwrap());
                            if starting_epoch != ending_epoch {
                                warn!(
                                    "Minimized snapshot range crosses epoch boundary ({} to \
                                         {}). Bank hashes after {} will not match replays from a \
                                         full snapshot",
                                    starting_epoch,
                                    ending_epoch,
                                    bank.epoch_schedule().get_last_slot_in_epoch(starting_epoch)
                                );
                            }

                            if minimize_snapshot_possibly_incomplete {
                                warn!(
                                    "Minimized snapshot may be incomplete due to missing \
                                         accounts from CPI'd address lookup table extensions. \
                                         This may lead to mismatched bank hashes while replaying."
                                );
                            }
                        }
                    }

                    if let Some(msg) = capitalization_message {
                        println!("{msg}");
                    }
                    println!(
                        "Shred version: {}",
                        compute_shred_version(&genesis_config.hash(), Some(&bank.hard_forks()))
                    );
                }
                ("accounts", Some(arg_matches)) => {
                    let process_options = parse_process_options(&ledger_path, arg_matches);
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let blockstore = open_blockstore(
                        &ledger_path,
                        arg_matches,
                        get_access_type(&process_options),
                    );
                    let (bank_forks, _) = load_and_process_ledger_or_exit(
                        arg_matches,
                        &genesis_config,
                        Arc::new(blockstore),
                        process_options,
                        snapshot_archive_path,
                        incremental_snapshot_archive_path,
                    );
                    let bank = bank_forks.read().unwrap().working_bank();

                    let include_sysvars = arg_matches.is_present("include_sysvars");
                    let include_account_contents = !arg_matches.is_present("no_account_contents");
                    let include_account_data = !arg_matches.is_present("no_account_data");
                    let account_data_encoding = parse_encoding_format(arg_matches);
                    let mode = if let Some(pubkeys) = pubkeys_of(arg_matches, "account") {
                        info!("Scanning individual accounts: {pubkeys:?}");
                        AccountsOutputMode::Individual(pubkeys)
                    } else if let Some(pubkey) = pubkey_of(arg_matches, "program_accounts") {
                        info!("Scanning program accounts for {pubkey}");
                        AccountsOutputMode::Program(pubkey)
                    } else {
                        info!("Scanning all accounts");
                        AccountsOutputMode::All
                    };
                    let config = AccountsOutputConfig {
                        mode,
                        include_sysvars,
                        include_account_contents,
                        include_account_data,
                        account_data_encoding,
                    };
                    let output_format =
                        OutputFormat::from_matches(arg_matches, "output_format", false);

                    let accounts_streamer =
                        AccountsOutputStreamer::new(bank, output_format, config);
                    let (_, scan_time) = measure!(
                        accounts_streamer
                            .output()
                            .map_err(|err| error!("Error while outputting accounts: {err}")),
                        "accounts scan"
                    );
                    info!("{scan_time}");
                }
                ("capitalization", Some(arg_matches)) => {
                    let process_options = parse_process_options(&ledger_path, arg_matches);
                    let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                    let blockstore = open_blockstore(
                        &ledger_path,
                        arg_matches,
                        get_access_type(&process_options),
                    );
                    let (bank_forks, _) = load_and_process_ledger_or_exit(
                        arg_matches,
                        &genesis_config,
                        Arc::new(blockstore),
                        process_options,
                        snapshot_archive_path,
                        incremental_snapshot_archive_path,
                    );
                    let bank_forks = bank_forks.read().unwrap();
                    let slot = bank_forks.working_bank().slot();
                    let bank = bank_forks.get(slot).unwrap_or_else(|| {
                        eprintln!("Error: Slot {slot} is not available");
                        exit(1);
                    });

                    if arg_matches.is_present("recalculate_capitalization") {
                        println!("Recalculating capitalization");
                        let old_capitalization = bank.set_capitalization();
                        if old_capitalization == bank.capitalization() {
                            eprintln!("Capitalization was identical: {}", Sol(old_capitalization));
                        }
                    }

                    if arg_matches.is_present("warp_epoch") {
                        let base_bank = bank;

                        let raw_warp_epoch = value_t!(arg_matches, "warp_epoch", String).unwrap();
                        let warp_epoch = if raw_warp_epoch.starts_with('+') {
                            base_bank.epoch() + value_t!(arg_matches, "warp_epoch", Epoch).unwrap()
                        } else {
                            value_t!(arg_matches, "warp_epoch", Epoch).unwrap()
                        };
                        if warp_epoch < base_bank.epoch() {
                            eprintln!(
                                "Error: can't warp epoch backwards: {} => {}",
                                base_bank.epoch(),
                                warp_epoch
                            );
                            exit(1);
                        }

                        if let Ok(raw_inflation) = value_t!(arg_matches, "inflation", String) {
                            let inflation = match raw_inflation.as_str() {
                                "pico" => Inflation::pico(),
                                "full" => Inflation::full(),
                                "none" => Inflation::new_disabled(),
                                _ => unreachable!(),
                            };
                            println!(
                                "Forcing to: {:?} (was: {:?})",
                                inflation,
                                base_bank.inflation()
                            );
                            base_bank.set_inflation(inflation);
                        }

                        let next_epoch = base_bank
                            .epoch_schedule()
                            .get_first_slot_in_epoch(warp_epoch);
                        // disable eager rent collection because this creates many unrelated
                        // rent collection account updates
                        base_bank
                            .lazy_rent_collection
                            .store(true, std::sync::atomic::Ordering::Relaxed);

                        let feature_account_balance = std::cmp::max(
                            genesis_config.rent.minimum_balance(Feature::size_of()),
                            1,
                        );
                        if arg_matches.is_present("enable_credits_auto_rewind") {
                            base_bank.unfreeze_for_ledger_tool();
                            let mut force_enabled_count = 0;
                            if base_bank
                                .get_account(&feature_set::credits_auto_rewind::id())
                                .is_none()
                            {
                                base_bank.store_account(
                                    &feature_set::credits_auto_rewind::id(),
                                    &feature::create_account(
                                        &Feature { activated_at: None },
                                        feature_account_balance,
                                    ),
                                );
                                force_enabled_count += 1;
                            }
                            if force_enabled_count == 0 {
                                warn!("Already credits_auto_rewind is activated (or scheduled)");
                            }
                            let mut store_failed_count = 0;
                            if force_enabled_count >= 1 {
                                if base_bank
                                    .get_account(&feature_set::deprecate_rewards_sysvar::id())
                                    .is_some()
                                {
                                    // steal some lamports from the pretty old feature not to affect
                                    // capitalizaion, which doesn't affect inflation behavior!
                                    base_bank.store_account(
                                        &feature_set::deprecate_rewards_sysvar::id(),
                                        &AccountSharedData::default(),
                                    );
                                    force_enabled_count -= 1;
                                } else {
                                    store_failed_count += 1;
                                }
                            }
                            assert_eq!(force_enabled_count, store_failed_count);
                            if store_failed_count >= 1 {
                                // we have no choice; maybe locally created blank cluster with
                                // not-Development cluster type.
                                let old_cap = base_bank.set_capitalization();
                                let new_cap = base_bank.capitalization();
                                warn!(
                                    "Skewing capitalization a bit to enable \
                                         credits_auto_rewind as requested: increasing {} from {} \
                                         to {}",
                                    feature_account_balance, old_cap, new_cap,
                                );
                                assert_eq!(
                                    old_cap + feature_account_balance * store_failed_count,
                                    new_cap
                                );
                            }
                        }

                        #[derive(Default, Debug)]
                        struct PointDetail {
                            epoch: Epoch,
                            points: u128,
                            stake: u128,
                            credits: u128,
                        }

                        #[derive(Default, Debug)]
                        struct CalculationDetail {
                            epochs: usize,
                            voter: Pubkey,
                            voter_owner: Pubkey,
                            current_effective_stake: u64,
                            total_stake: u64,
                            rent_exempt_reserve: u64,
                            points: Vec<PointDetail>,
                            base_rewards: u64,
                            commission: u8,
                            vote_rewards: u64,
                            stake_rewards: u64,
                            activation_epoch: Epoch,
                            deactivation_epoch: Option<Epoch>,
                            point_value: Option<PointValue>,
                            old_credits_observed: Option<u64>,
                            new_credits_observed: Option<u64>,
                            skipped_reasons: String,
                        }
                        use solana_stake_program::points::InflationPointCalculationEvent;
                        let stake_calculation_details: DashMap<Pubkey, CalculationDetail> =
                            DashMap::new();
                        let last_point_value = Arc::new(RwLock::new(None));
                        let tracer = |event: &RewardCalculationEvent| {
                            // Currently RewardCalculationEvent enum has only Staking variant
                            // because only staking tracing is supported!
                            #[allow(irrefutable_let_patterns)]
                            if let RewardCalculationEvent::Staking(pubkey, event) = event {
                                let mut detail =
                                    stake_calculation_details.entry(**pubkey).or_default();
                                match event {
                                InflationPointCalculationEvent::CalculatedPoints(
                                    epoch,
                                    stake,
                                    credits,
                                    points,
                                ) => {
                                    if *points > 0 {
                                        detail.epochs += 1;
                                        detail.points.push(PointDetail {
                                            epoch: *epoch,
                                            points: *points,
                                            stake: *stake,
                                            credits: *credits,
                                        });
                                    }
                                }
                                InflationPointCalculationEvent::SplitRewards(
                                    all,
                                    voter,
                                    staker,
                                    point_value,
                                ) => {
                                    detail.base_rewards = *all;
                                    detail.vote_rewards = *voter;
                                    detail.stake_rewards = *staker;
                                    detail.point_value = Some(point_value.clone());
                                    // we have duplicate copies of `PointValue`s for possible
                                    // miscalculation; do some minimum sanity check
                                    let mut last_point_value = last_point_value.write().unwrap();
                                    if let Some(last_point_value) = last_point_value.as_ref() {
                                        assert_eq!(last_point_value, point_value);
                                    } else {
                                        *last_point_value = Some(point_value.clone());
                                    }
                                }
                                InflationPointCalculationEvent::EffectiveStakeAtRewardedEpoch(
                                    stake,
                                ) => {
                                    detail.current_effective_stake = *stake;
                                }
                                InflationPointCalculationEvent::Commission(commission) => {
                                    detail.commission = *commission;
                                }
                                InflationPointCalculationEvent::RentExemptReserve(reserve) => {
                                    detail.rent_exempt_reserve = *reserve;
                                }
                                InflationPointCalculationEvent::CreditsObserved(
                                    old_credits_observed,
                                    new_credits_observed,
                                ) => {
                                    detail.old_credits_observed = Some(*old_credits_observed);
                                    detail.new_credits_observed = *new_credits_observed;
                                }
                                InflationPointCalculationEvent::Delegation(delegation, owner) => {
                                    detail.voter = delegation.voter_pubkey;
                                    detail.voter_owner = *owner;
                                    detail.total_stake = delegation.stake;
                                    detail.activation_epoch = delegation.activation_epoch;
                                    if delegation.deactivation_epoch < Epoch::max_value() {
                                        detail.deactivation_epoch =
                                            Some(delegation.deactivation_epoch);
                                    }
                                }
                                InflationPointCalculationEvent::Skipped(skipped_reason) => {
                                    if detail.skipped_reasons.is_empty() {
                                        detail.skipped_reasons = format!("{skipped_reason:?}");
                                    } else {
                                        use std::fmt::Write;
                                        let _ = write!(
                                            &mut detail.skipped_reasons,
                                            "/{skipped_reason:?}"
                                        );
                                    }
                                }
                            }
                            }
                        };
                        let warped_bank = Bank::new_from_parent_with_tracer(
                            base_bank.clone(),
                            base_bank.collector_id(),
                            next_epoch,
                            tracer,
                        );
                        warped_bank.freeze();
                        let mut csv_writer = if arg_matches.is_present("csv_filename") {
                            let csv_filename =
                                value_t_or_exit!(arg_matches, "csv_filename", String);
                            let file = File::create(csv_filename).unwrap();
                            Some(csv::WriterBuilder::new().from_writer(file))
                        } else {
                            None
                        };

                        println!("Slot: {} => {}", base_bank.slot(), warped_bank.slot());
                        println!("Epoch: {} => {}", base_bank.epoch(), warped_bank.epoch());
                        assert_capitalization(&base_bank);
                        assert_capitalization(&warped_bank);
                        let interest_per_epoch = ((warped_bank.capitalization() as f64)
                            / (base_bank.capitalization() as f64)
                            * 100_f64)
                            - 100_f64;
                        let interest_per_year = interest_per_epoch
                            / warped_bank.epoch_duration_in_years(base_bank.epoch());
                        println!(
                            "Capitalization: {} => {} (+{} {}%; annualized {}%)",
                            Sol(base_bank.capitalization()),
                            Sol(warped_bank.capitalization()),
                            Sol(warped_bank.capitalization() - base_bank.capitalization()),
                            interest_per_epoch,
                            interest_per_year,
                        );

                        let mut overall_delta = 0;

                        let modified_accounts =
                            warped_bank.get_all_accounts_modified_since_parent();
                        let mut rewarded_accounts = modified_accounts
                            .iter()
                            .map(|(pubkey, account)| {
                                (
                                    pubkey,
                                    account,
                                    base_bank
                                        .get_account(pubkey)
                                        .map(|a| a.lamports())
                                        .unwrap_or_default(),
                                )
                            })
                            .collect::<Vec<_>>();
                        rewarded_accounts.sort_unstable_by_key(
                            |(pubkey, account, base_lamports)| {
                                (
                                    *account.owner(),
                                    *base_lamports,
                                    account.lamports() - base_lamports,
                                    *pubkey,
                                )
                            },
                        );

                        let mut unchanged_accounts = stake_calculation_details
                            .iter()
                            .map(|entry| *entry.key())
                            .collect::<HashSet<_>>()
                            .difference(
                                &rewarded_accounts
                                    .iter()
                                    .map(|(pubkey, ..)| **pubkey)
                                    .collect(),
                            )
                            .map(|pubkey| (*pubkey, warped_bank.get_account(pubkey).unwrap()))
                            .collect::<Vec<_>>();
                        unchanged_accounts.sort_unstable_by_key(|(pubkey, account)| {
                            (*account.owner(), account.lamports(), *pubkey)
                        });
                        let unchanged_accounts = unchanged_accounts.into_iter();

                        let rewarded_accounts = rewarded_accounts
                            .into_iter()
                            .map(|(pubkey, account, ..)| (*pubkey, account.clone()));

                        let all_accounts = unchanged_accounts.chain(rewarded_accounts);
                        for (pubkey, warped_account) in all_accounts {
                            // Don't output sysvars; it's always updated but not related to
                            // inflation.
                            if solana_sdk::sysvar::is_sysvar_id(&pubkey) {
                                continue;
                            }

                            if let Some(base_account) = base_bank.get_account(&pubkey) {
                                let delta = warped_account.lamports() - base_account.lamports();
                                let detail_ref = stake_calculation_details.get(&pubkey);
                                let detail: Option<&CalculationDetail> =
                                    detail_ref.as_ref().map(|detail_ref| detail_ref.value());
                                println!(
                                    "{:<45}({}): {} => {} (+{} {:>4.9}%) {:?}",
                                    format!("{pubkey}"), // format! is needed to pad/justify correctly.
                                    base_account.owner(),
                                    Sol(base_account.lamports()),
                                    Sol(warped_account.lamports()),
                                    Sol(delta),
                                    ((warped_account.lamports() as f64)
                                        / (base_account.lamports() as f64)
                                        * 100_f64)
                                        - 100_f64,
                                    detail,
                                );
                                if let Some(ref mut csv_writer) = csv_writer {
                                    #[derive(Serialize)]
                                    struct InflationRecord {
                                        cluster_type: String,
                                        rewarded_epoch: Epoch,
                                        account: String,
                                        owner: String,
                                        old_balance: u64,
                                        new_balance: u64,
                                        data_size: usize,
                                        delegation: String,
                                        delegation_owner: String,
                                        effective_stake: String,
                                        delegated_stake: String,
                                        rent_exempt_reserve: String,
                                        activation_epoch: String,
                                        deactivation_epoch: String,
                                        earned_epochs: String,
                                        epoch: String,
                                        epoch_credits: String,
                                        epoch_points: String,
                                        epoch_stake: String,
                                        old_credits_observed: String,
                                        new_credits_observed: String,
                                        base_rewards: String,
                                        stake_rewards: String,
                                        vote_rewards: String,
                                        commission: String,
                                        cluster_rewards: String,
                                        cluster_points: String,
                                        old_capitalization: u64,
                                        new_capitalization: u64,
                                    }
                                    fn format_or_na<T: std::fmt::Display>(
                                        data: Option<T>,
                                    ) -> String {
                                        data.map(|data| format!("{data}"))
                                            .unwrap_or_else(|| "N/A".to_owned())
                                    }
                                    let mut point_details = detail
                                        .map(|d| d.points.iter().map(Some).collect::<Vec<_>>())
                                        .unwrap_or_default();

                                    // ensure to print even if there is no calculation/point detail
                                    if point_details.is_empty() {
                                        point_details.push(None);
                                    }

                                    for point_detail in point_details {
                                        let (cluster_rewards, cluster_points) = last_point_value
                                            .read()
                                            .unwrap()
                                            .clone()
                                            .map_or((None, None), |pv| {
                                                (Some(pv.rewards), Some(pv.points))
                                            });
                                        let record = InflationRecord {
                                            cluster_type: format!("{:?}", base_bank.cluster_type()),
                                            rewarded_epoch: base_bank.epoch(),
                                            account: format!("{pubkey}"),
                                            owner: format!("{}", base_account.owner()),
                                            old_balance: base_account.lamports(),
                                            new_balance: warped_account.lamports(),
                                            data_size: base_account.data().len(),
                                            delegation: format_or_na(detail.map(|d| d.voter)),
                                            delegation_owner: format_or_na(
                                                detail.map(|d| d.voter_owner),
                                            ),
                                            effective_stake: format_or_na(
                                                detail.map(|d| d.current_effective_stake),
                                            ),
                                            delegated_stake: format_or_na(
                                                detail.map(|d| d.total_stake),
                                            ),
                                            rent_exempt_reserve: format_or_na(
                                                detail.map(|d| d.rent_exempt_reserve),
                                            ),
                                            activation_epoch: format_or_na(detail.map(|d| {
                                                if d.activation_epoch < Epoch::max_value() {
                                                    d.activation_epoch
                                                } else {
                                                    // bootstraped
                                                    0
                                                }
                                            })),
                                            deactivation_epoch: format_or_na(
                                                detail.and_then(|d| d.deactivation_epoch),
                                            ),
                                            earned_epochs: format_or_na(detail.map(|d| d.epochs)),
                                            epoch: format_or_na(point_detail.map(|d| d.epoch)),
                                            epoch_credits: format_or_na(
                                                point_detail.map(|d| d.credits),
                                            ),
                                            epoch_points: format_or_na(
                                                point_detail.map(|d| d.points),
                                            ),
                                            epoch_stake: format_or_na(
                                                point_detail.map(|d| d.stake),
                                            ),
                                            old_credits_observed: format_or_na(
                                                detail.and_then(|d| d.old_credits_observed),
                                            ),
                                            new_credits_observed: format_or_na(
                                                detail.and_then(|d| d.new_credits_observed),
                                            ),
                                            base_rewards: format_or_na(
                                                detail.map(|d| d.base_rewards),
                                            ),
                                            stake_rewards: format_or_na(
                                                detail.map(|d| d.stake_rewards),
                                            ),
                                            vote_rewards: format_or_na(
                                                detail.map(|d| d.vote_rewards),
                                            ),
                                            commission: format_or_na(detail.map(|d| d.commission)),
                                            cluster_rewards: format_or_na(cluster_rewards),
                                            cluster_points: format_or_na(cluster_points),
                                            old_capitalization: base_bank.capitalization(),
                                            new_capitalization: warped_bank.capitalization(),
                                        };
                                        csv_writer.serialize(&record).unwrap();
                                    }
                                }
                                overall_delta += delta;
                            } else {
                                error!("new account!?: {}", pubkey);
                            }
                        }
                        if overall_delta > 0 {
                            println!("Sum of lamports changes: {}", Sol(overall_delta));
                        }
                    } else {
                        if arg_matches.is_present("recalculate_capitalization") {
                            eprintln!("Capitalization isn't verified because it's recalculated");
                        }
                        if arg_matches.is_present("inflation") {
                            eprintln!(
                                "Forcing inflation isn't meaningful because bank isn't warping"
                            );
                        }

                        assert_capitalization(&bank);
                        println!("Inflation: {:?}", bank.inflation());
                        println!("RentCollector: {:?}", bank.rent_collector());
                        println!("Capitalization: {}", Sol(bank.capitalization()));
                    }
                }
                ("compute-slot-cost", Some(arg_matches)) => {
                    let blockstore =
                        open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);

                    let mut slots: Vec<u64> = vec![];
                    if !arg_matches.is_present("slots") {
                        if let Ok(metas) = blockstore.slot_meta_iterator(0) {
                            slots = metas.map(|(slot, _)| slot).collect();
                        }
                    } else {
                        slots = values_t_or_exit!(arg_matches, "slots", Slot);
                    }
                    let allow_dead_slots = arg_matches.is_present("allow_dead_slots");

                    for slot in slots {
                        if let Err(err) = compute_slot_cost(&blockstore, slot, allow_dead_slots) {
                            eprintln!("{err}");
                        }
                    }
                }
                ("", _) => {
                    eprintln!("{}", matches.usage());
                    exit(1);
                }
                _ => unreachable!(),
            };
        }
    };
    measure_total_execution_time.stop();
    info!("{}", measure_total_execution_time);
}
