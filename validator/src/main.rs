#![allow(clippy::integer_arithmetic)]
#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;
use {
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, values_t, values_t_or_exit, App,
        AppSettings, Arg, ArgMatches, SubCommand,
    },
    console::style,
    log::*,
    rand::{seq::SliceRandom, thread_rng},
    solana_clap_utils::{
        input_parsers::{keypair_of, keypairs_of, pubkey_of, value_of},
        input_validators::{
            is_keypair, is_keypair_or_ask_keyword, is_niceness_adjustment_valid, is_parsable,
            is_pow2, is_pubkey, is_pubkey_or_keypair, is_slot, is_valid_percentage,
            is_within_range,
        },
        keypair::SKIP_SEED_PHRASE_VALIDATION_ARG,
    },
    solana_core::{
        ledger_cleanup_service::{DEFAULT_MAX_LEDGER_SHREDS, DEFAULT_MIN_MAX_LEDGER_SHREDS},
        system_monitor_service::SystemMonitorService,
        tower_storage,
        tpu::DEFAULT_TPU_COALESCE_MS,
        validator::{is_snapshot_config_valid, Validator, ValidatorConfig, ValidatorStartProgress},
    },
    solana_gossip::{cluster_info::Node, contact_info::ContactInfo},
    solana_ledger::blockstore_options::{
        BlockstoreCompressionType, BlockstoreRecoveryMode, LedgerColumnOptions, ShredStorageType,
        DEFAULT_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES,
    },
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_perf::recycler::enable_recycler_warming,
    solana_poh::poh_service,
    solana_rpc::{
        rpc::{JsonRpcConfig, RpcBigtableConfig, MAX_REQUEST_BODY_SIZE},
        rpc_pubsub_service::PubSubConfig,
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{config::RpcLeaderScheduleConfig, request::MAX_MULTIPLE_ACCOUNTS},
    solana_runtime::{
        accounts_db::{
            AccountShrinkThreshold, AccountsDbConfig, FillerAccountsConfig,
            DEFAULT_ACCOUNTS_SHRINK_OPTIMIZE_TOTAL_SPACE, DEFAULT_ACCOUNTS_SHRINK_RATIO,
        },
        accounts_index::{
            AccountIndex, AccountSecondaryIndexes, AccountSecondaryIndexesIncludeExclude,
            AccountsIndexConfig, IndexLimitMb,
        },
        hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
        runtime_config::RuntimeConfig,
        snapshot_config::{SnapshotConfig, SnapshotUsage},
        snapshot_utils::{
            self, ArchiveFormat, SnapshotVersion, DEFAULT_ARCHIVE_COMPRESSION,
            DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
            DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN, SUPPORTED_ARCHIVE_COMPRESSION,
        },
    },
    solana_sdk::{
        clock::{Slot, DEFAULT_S_PER_SLOT},
        commitment_config::CommitmentConfig,
        hash::Hash,
        pubkey::Pubkey,
        signature::{read_keypair, Keypair, Signer},
    },
    solana_send_transaction_service::send_transaction_service::{
        self, MAX_BATCH_SEND_RATE_MS, MAX_TRANSACTION_BATCH_SIZE,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_tpu_client::connection_cache::{
        DEFAULT_TPU_CONNECTION_POOL_SIZE, DEFAULT_TPU_ENABLE_UDP,
    },
    solana_validator::{
        admin_rpc_service,
        admin_rpc_service::{load_staked_nodes_overrides, StakedNodesOverrides},
        bootstrap,
        dashboard::Dashboard,
        ledger_lockfile, lock_ledger, new_spinner_progress_bar, println_name_value,
        redirect_stderr_to_file,
    },
    std::{
        collections::{HashSet, VecDeque},
        env,
        fs::{self, File},
        net::{IpAddr, SocketAddr},
        path::{Path, PathBuf},
        process::exit,
        str::FromStr,
        sync::{Arc, RwLock},
        time::{Duration, SystemTime},
    },
};

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[derive(Debug, PartialEq, Eq)]
enum Operation {
    Initialize,
    Run,
}

const EXCLUDE_KEY: &str = "account-index-exclude-key";
const INCLUDE_KEY: &str = "account-index-include-key";
// The default minimal snapshot download speed (bytes/second)
const DEFAULT_MIN_SNAPSHOT_DOWNLOAD_SPEED: u64 = 10485760;
// The maximum times of snapshot download abort and retry
const MAX_SNAPSHOT_DOWNLOAD_ABORT: u32 = 5;
const MILLIS_PER_SECOND: u64 = 1000;

fn monitor_validator(ledger_path: &Path) {
    let dashboard = Dashboard::new(ledger_path, None, None).unwrap_or_else(|err| {
        println!(
            "Error: Unable to connect to validator at {}: {:?}",
            ledger_path.display(),
            err,
        );
        exit(1);
    });
    dashboard.run(Duration::from_secs(2));
}

fn wait_for_restart_window(
    ledger_path: &Path,
    identity: Option<Pubkey>,
    min_idle_time_in_minutes: usize,
    max_delinquency_percentage: u8,
    skip_new_snapshot_check: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let sleep_interval = Duration::from_secs(5);

    let min_idle_slots = (min_idle_time_in_minutes as f64 * 60. / DEFAULT_S_PER_SLOT) as Slot;

    let admin_client = admin_rpc_service::connect(ledger_path);
    let rpc_addr = admin_rpc_service::runtime()
        .block_on(async move { admin_client.await?.rpc_addr().await })
        .map_err(|err| format!("Unable to get validator RPC address: {}", err))?;

    let rpc_client = match rpc_addr {
        None => return Err("RPC not available".into()),
        Some(rpc_addr) => RpcClient::new_socket(rpc_addr),
    };

    let my_identity = rpc_client.get_identity()?;
    let identity = identity.unwrap_or(my_identity);
    let monitoring_another_validator = identity != my_identity;
    println_name_value("Identity:", &identity.to_string());
    println_name_value(
        "Minimum Idle Time:",
        &format!(
            "{} slots (~{} minutes)",
            min_idle_slots, min_idle_time_in_minutes
        ),
    );

    println!(
        "Maximum permitted delinquency: {}%",
        max_delinquency_percentage
    );

    let mut current_epoch = None;
    let mut leader_schedule = VecDeque::new();
    let mut restart_snapshot = None;
    let mut upcoming_idle_windows = vec![]; // Vec<(starting slot, idle window length in slots)>

    let progress_bar = new_spinner_progress_bar();
    let monitor_start_time = SystemTime::now();

    let mut seen_incremential_snapshot = false;
    loop {
        let snapshot_slot_info = rpc_client.get_highest_snapshot_slot().ok();
        let snapshot_slot_info_has_incremential = snapshot_slot_info
            .as_ref()
            .map(|snapshot_slot_info| snapshot_slot_info.incremental.is_some())
            .unwrap_or_default();
        seen_incremential_snapshot |= snapshot_slot_info_has_incremential;

        let epoch_info = rpc_client.get_epoch_info_with_commitment(CommitmentConfig::processed())?;
        let healthy = rpc_client.get_health().ok().is_some();
        let delinquent_stake_percentage = {
            let vote_accounts = rpc_client.get_vote_accounts()?;
            let current_stake: u64 = vote_accounts
                .current
                .iter()
                .map(|va| va.activated_stake)
                .sum();
            let delinquent_stake: u64 = vote_accounts
                .delinquent
                .iter()
                .map(|va| va.activated_stake)
                .sum();
            let total_stake = current_stake + delinquent_stake;
            delinquent_stake as f64 / total_stake as f64
        };

        if match current_epoch {
            None => true,
            Some(current_epoch) => current_epoch != epoch_info.epoch,
        } {
            progress_bar.set_message(format!(
                "Fetching leader schedule for epoch {}...",
                epoch_info.epoch
            ));
            let first_slot_in_epoch = epoch_info.absolute_slot - epoch_info.slot_index;
            leader_schedule = rpc_client
                .get_leader_schedule_with_config(
                    Some(first_slot_in_epoch),
                    RpcLeaderScheduleConfig {
                        identity: Some(identity.to_string()),
                        ..RpcLeaderScheduleConfig::default()
                    },
                )?
                .ok_or_else(|| {
                    format!(
                        "Unable to get leader schedule from slot {}",
                        first_slot_in_epoch
                    )
                })?
                .get(&identity.to_string())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|slot_index| first_slot_in_epoch.saturating_add(slot_index as u64))
                .filter(|slot| *slot > epoch_info.absolute_slot)
                .collect::<VecDeque<_>>();

            upcoming_idle_windows.clear();
            {
                let mut leader_schedule = leader_schedule.clone();
                let mut max_idle_window = 0;

                let mut idle_window_start_slot = epoch_info.absolute_slot;
                while let Some(next_leader_slot) = leader_schedule.pop_front() {
                    let idle_window = next_leader_slot - idle_window_start_slot;
                    max_idle_window = max_idle_window.max(idle_window);
                    if idle_window > min_idle_slots {
                        upcoming_idle_windows.push((idle_window_start_slot, idle_window));
                    }
                    idle_window_start_slot = next_leader_slot;
                }
                if !leader_schedule.is_empty() && upcoming_idle_windows.is_empty() {
                    return Err(format!(
                        "Validator has no idle window of at least {} slots. Largest idle window for epoch {} is {} slots",
                        min_idle_slots, epoch_info.epoch, max_idle_window
                    )
                    .into());
                }
            }

            current_epoch = Some(epoch_info.epoch);
        }

        let status = {
            if !healthy {
                style("Node is unhealthy").red().to_string()
            } else {
                // Wait until a hole in the leader schedule before restarting the node
                let in_leader_schedule_hole = if epoch_info.slot_index + min_idle_slots as u64
                    > epoch_info.slots_in_epoch
                {
                    Err("Current epoch is almost complete".to_string())
                } else {
                    while leader_schedule
                        .get(0)
                        .map(|slot| *slot < epoch_info.absolute_slot)
                        .unwrap_or(false)
                    {
                        leader_schedule.pop_front();
                    }
                    while upcoming_idle_windows
                        .first()
                        .map(|(slot, _)| *slot < epoch_info.absolute_slot)
                        .unwrap_or(false)
                    {
                        upcoming_idle_windows.pop();
                    }

                    match leader_schedule.get(0) {
                        None => {
                            Ok(()) // Validator has no leader slots
                        }
                        Some(next_leader_slot) => {
                            let idle_slots =
                                next_leader_slot.saturating_sub(epoch_info.absolute_slot);
                            if idle_slots >= min_idle_slots {
                                Ok(())
                            } else {
                                Err(match upcoming_idle_windows.first() {
                                    Some((starting_slot, length_in_slots)) => {
                                        format!(
                                            "Next idle window in {} slots, for {} slots",
                                            starting_slot.saturating_sub(epoch_info.absolute_slot),
                                            length_in_slots
                                        )
                                    }
                                    None => format!(
                                        "Validator will be leader soon. Next leader slot is {}",
                                        next_leader_slot
                                    ),
                                })
                            }
                        }
                    }
                };

                match in_leader_schedule_hole {
                    Ok(_) => {
                        if skip_new_snapshot_check {
                            break; // Restart!
                        }
                        let snapshot_slot = snapshot_slot_info.map(|snapshot_slot_info| {
                            snapshot_slot_info
                                .incremental
                                .unwrap_or(snapshot_slot_info.full)
                        });
                        if restart_snapshot.is_none() {
                            restart_snapshot = snapshot_slot;
                        }
                        if restart_snapshot == snapshot_slot && !monitoring_another_validator {
                            "Waiting for a new snapshot".to_string()
                        } else if delinquent_stake_percentage
                            >= (max_delinquency_percentage as f64 / 100.)
                        {
                            style("Delinquency too high").red().to_string()
                        } else if seen_incremential_snapshot && !snapshot_slot_info_has_incremential
                        {
                            // Restarts using just a full snapshot will put the node significantly
                            // further behind than if an incremental snapshot is also used, as full
                            // snapshots are larger and take much longer to create.
                            //
                            // Therefore if the node just created a new full snapshot, wait a
                            // little longer until it creates the first incremental snapshot for
                            // the full snapshot.
                            "Waiting for incremental snapshot".to_string()
                        } else {
                            break; // Restart!
                        }
                    }
                    Err(why) => style(why).yellow().to_string(),
                }
            }
        };

        progress_bar.set_message(format!(
            "{} | Processed Slot: {} {} | {:.2}% delinquent stake | {}",
            {
                let elapsed =
                    chrono::Duration::from_std(monitor_start_time.elapsed().unwrap()).unwrap();

                format!(
                    "{:02}:{:02}:{:02}",
                    elapsed.num_hours(),
                    elapsed.num_minutes() % 60,
                    elapsed.num_seconds() % 60
                )
            },
            epoch_info.absolute_slot,
            if monitoring_another_validator {
                "".to_string()
            } else {
                format!(
                    "| Full Snapshot Slot: {} | Incremental Snapshot Slot: {}",
                    snapshot_slot_info
                        .as_ref()
                        .map(|snapshot_slot_info| snapshot_slot_info.full.to_string())
                        .unwrap_or_else(|| '-'.to_string()),
                    snapshot_slot_info
                        .as_ref()
                        .and_then(|snapshot_slot_info| snapshot_slot_info
                            .incremental
                            .map(|incremental| incremental.to_string()))
                        .unwrap_or_else(|| '-'.to_string()),
                )
            },
            delinquent_stake_percentage * 100.,
            status
        ));
        std::thread::sleep(sleep_interval);
    }
    drop(progress_bar);
    println!("{}", style("Ready to restart").green());
    Ok(())
}

fn hash_validator(hash: String) -> Result<(), String> {
    Hash::from_str(&hash)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

// This function is duplicated in ledger-tool/src/main.rs...
fn hardforks_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<Slot>> {
    if matches.is_present(name) {
        Some(values_t_or_exit!(matches, name, Slot))
    } else {
        None
    }
}

fn validators_set(
    identity_pubkey: &Pubkey,
    matches: &ArgMatches<'_>,
    matches_name: &str,
    arg_name: &str,
) -> Option<HashSet<Pubkey>> {
    if matches.is_present(matches_name) {
        let validators_set: HashSet<_> = values_t_or_exit!(matches, matches_name, Pubkey)
            .into_iter()
            .collect();
        if validators_set.contains(identity_pubkey) {
            eprintln!(
                "The validator's identity pubkey cannot be a {}: {}",
                arg_name, identity_pubkey
            );
            exit(1);
        }
        Some(validators_set)
    } else {
        None
    }
}

fn get_cluster_shred_version(entrypoints: &[SocketAddr]) -> Option<u16> {
    let entrypoints = {
        let mut index: Vec<_> = (0..entrypoints.len()).collect();
        index.shuffle(&mut rand::thread_rng());
        index.into_iter().map(|i| &entrypoints[i])
    };
    for entrypoint in entrypoints {
        match solana_net_utils::get_cluster_shred_version(entrypoint) {
            Err(err) => eprintln!("get_cluster_shred_version failed: {}, {}", entrypoint, err),
            Ok(0) => eprintln!("zero shred-version from entrypoint: {}", entrypoint),
            Ok(shred_version) => {
                info!(
                    "obtained shred-version {} from {}",
                    shred_version, entrypoint
                );
                return Some(shred_version);
            }
        }
    }
    None
}

pub fn main() {
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    let default_genesis_archive_unpacked_size = &MAX_GENESIS_ARCHIVE_UNPACKED_SIZE.to_string();
    let default_rpc_max_multiple_accounts = &MAX_MULTIPLE_ACCOUNTS.to_string();

    let default_rpc_pubsub_max_active_subscriptions =
        PubSubConfig::default().max_active_subscriptions.to_string();
    let default_rpc_pubsub_queue_capacity_items =
        PubSubConfig::default().queue_capacity_items.to_string();
    let default_rpc_pubsub_queue_capacity_bytes =
        PubSubConfig::default().queue_capacity_bytes.to_string();
    let default_send_transaction_service_config = send_transaction_service::Config::default();
    let default_rpc_send_transaction_retry_ms = default_send_transaction_service_config
        .retry_rate_ms
        .to_string();
    let default_rpc_send_transaction_batch_ms = default_send_transaction_service_config
        .batch_send_rate_ms
        .to_string();
    let default_rpc_send_transaction_leader_forward_count = default_send_transaction_service_config
        .leader_forward_count
        .to_string();
    let default_rpc_send_transaction_service_max_retries = default_send_transaction_service_config
        .service_max_retries
        .to_string();
    let default_rpc_send_transaction_batch_size = default_send_transaction_service_config
        .batch_size
        .to_string();
    let default_rpc_threads = num_cpus::get().to_string();
    let default_accountsdb_repl_threads = num_cpus::get().to_string();
    let default_maximum_full_snapshot_archives_to_retain =
        &DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN.to_string();
    let default_maximum_incremental_snapshot_archives_to_retain =
        &DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN.to_string();
    let default_full_snapshot_archive_interval_slots =
        &DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS.to_string();
    let default_incremental_snapshot_archive_interval_slots =
        &DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS.to_string();
    let default_min_snapshot_download_speed = &DEFAULT_MIN_SNAPSHOT_DOWNLOAD_SPEED.to_string();
    let default_max_snapshot_download_abort = &MAX_SNAPSHOT_DOWNLOAD_ABORT.to_string();
    let default_accounts_shrink_optimize_total_space =
        &DEFAULT_ACCOUNTS_SHRINK_OPTIMIZE_TOTAL_SPACE.to_string();
    let default_accounts_shrink_ratio = &DEFAULT_ACCOUNTS_SHRINK_RATIO.to_string();
    let default_rocksdb_fifo_shred_storage_size =
        &DEFAULT_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES.to_string();
    let default_tpu_connection_pool_size = &DEFAULT_TPU_CONNECTION_POOL_SIZE.to_string();
    let default_rpc_max_request_body_size = &MAX_REQUEST_BODY_SIZE.to_string();

    let matches = App::new(crate_name!()).about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::InferSubcommands)
        .arg(
            Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
        )
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("KEYPAIR")
                .takes_value(true)
                .validator(is_keypair_or_ask_keyword)
                .help("Validator identity keypair"),
        )
        .arg(
            Arg::with_name("authorized_voter_keypairs")
                .long("authorized-voter")
                .value_name("KEYPAIR")
                .takes_value(true)
                .validator(is_keypair_or_ask_keyword)
                .requires("vote_account")
                .multiple(true)
                .help("Include an additional authorized voter keypair. \
                       May be specified multiple times. \
                       [default: the --identity keypair]"),
        )
        .arg(
            Arg::with_name("vote_account")
                .long("vote-account")
                .value_name("ADDRESS")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .requires("identity")
                .help("Validator vote account public key.  \
                       If unspecified voting will be disabled. \
                       The authorized voter for the account must either be the \
                       --identity keypair or with the --authorized-voter argument")
        )
        .arg(
            Arg::with_name("init_complete_file")
                .long("init-complete-file")
                .value_name("FILE")
                .takes_value(true)
                .help("Create this file if it doesn't already exist \
                       once validator initialization is complete"),
        )
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .default_value("ledger")
                .help("Use DIR as ledger location"),
        )
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .multiple(true)
                .validator(solana_net_utils::is_host_port)
                .help("Rendezvous with the cluster at this gossip entrypoint"),
        )
        .arg(
            Arg::with_name("no_snapshot_fetch")
                .long("no-snapshot-fetch")
                .takes_value(false)
                .help("Do not attempt to fetch a snapshot from the cluster, \
                      start from a local snapshot if present"),
        )
        .arg(
            Arg::with_name("no_genesis_fetch")
                .long("no-genesis-fetch")
                .takes_value(false)
                .help("Do not fetch genesis from the cluster"),
        )
        .arg(
            Arg::with_name("no_voting")
                .long("no-voting")
                .takes_value(false)
                .help("Launch validator without voting"),
        )
        .arg(
            Arg::with_name("no_check_vote_account")
                .long("no-check-vote-account")
                .takes_value(false)
                .conflicts_with("no_voting")
                .requires("entrypoint")
                .hidden(true)
                .help("Skip the RPC vote account sanity check")
        )
        .arg(
            Arg::with_name("check_vote_account")
                .long("check-vote-account")
                .takes_value(true)
                .value_name("RPC_URL")
                .requires("entrypoint")
                .conflicts_with_all(&["no_check_vote_account", "no_voting"])
                .help("Sanity check vote account state at startup. The JSON RPC endpoint at RPC_URL must expose `--full-rpc-api`")
        )
        .arg(
            Arg::with_name("restricted_repair_only_mode")
                .long("restricted-repair-only-mode")
                .takes_value(false)
                .help("Do not publish the Gossip, TPU, TVU or Repair Service ports causing \
                       the validator to operate in a limited capacity that reduces its \
                       exposure to the rest of the cluster. \
                       \
                       The --no-voting flag is implicit when this flag is enabled \
                      "),
        )
        .arg(
            Arg::with_name("dev_halt_at_slot")
                .long("dev-halt-at-slot")
                .value_name("SLOT")
                .validator(is_slot)
                .takes_value(true)
                .help("Halt the validator when it reaches the given slot"),
        )
        .arg(
            Arg::with_name("rpc_port")
                .long("rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .validator(solana_validator::port_validator)
                .help("Enable JSON RPC on this port, and the next port for the RPC websocket"),
        )
        .arg(
            Arg::with_name("minimal_rpc_api")
                .long("--minimal-rpc-api")
                .takes_value(false)
                .hidden(true)
                .help("Only expose the RPC methods required to serve snapshots to other nodes"),
        )
        .arg(
            Arg::with_name("full_rpc_api")
                .long("--full-rpc-api")
                .conflicts_with("minimal_rpc_api")
                .takes_value(false)
                .help("Expose RPC methods for querying chain state and transaction history"),
        )
        .arg(
            Arg::with_name("obsolete_v1_7_rpc_api")
                .long("--enable-rpc-obsolete_v1_7")
                .takes_value(false)
                .help("Enable the obsolete RPC methods removed in v1.7"),
        )
        .arg(
            Arg::with_name("private_rpc")
                .long("--private-rpc")
                .takes_value(false)
                .help("Do not publish the RPC port for use by others")
        )
        .arg(
            Arg::with_name("no_port_check")
                .long("--no-port-check")
                .takes_value(false)
                .help("Do not perform TCP/UDP reachable port checks at start-up")
        )
        .arg(
            Arg::with_name("enable_rpc_transaction_history")
                .long("enable-rpc-transaction-history")
                .takes_value(false)
                .help("Enable historical transaction info over JSON RPC, \
                       including the 'getConfirmedBlock' API.  \
                       This will cause an increase in disk usage and IOPS"),
        )
        .arg(
            Arg::with_name("enable_rpc_bigtable_ledger_storage")
                .long("enable-rpc-bigtable-ledger-storage")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .help("Fetch historical transaction info from a BigTable instance \
                       as a fallback to local ledger data"),
        )
        .arg(
            Arg::with_name("enable_bigtable_ledger_upload")
                .long("enable-bigtable-ledger-upload")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .help("Upload new confirmed blocks into a BigTable instance"),
        )
        .arg(
            Arg::with_name("enable_cpi_and_log_storage")
                .long("enable-cpi-and-log-storage")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .hidden(true)
                .help("Deprecated, please use \"enable-extended-tx-metadata-storage\". \
                       Include CPI inner instructions, logs and return data in \
                       the historical transaction info stored"),
        )
        .arg(
            Arg::with_name("enable_extended_tx_metadata_storage")
                .long("enable-extended-tx-metadata-storage")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .help("Include CPI inner instructions, logs, and return data in \
                       the historical transaction info stored"),
        )
        .arg(
            Arg::with_name("rpc_max_multiple_accounts")
                .long("rpc-max-multiple-accounts")
                .value_name("MAX ACCOUNTS")
                .takes_value(true)
                .default_value(default_rpc_max_multiple_accounts)
                .help("Override the default maximum accounts accepted by \
                       the getMultipleAccounts JSON RPC method")
        )
        .arg(
            Arg::with_name("health_check_slot_distance")
                .long("health-check-slot-distance")
                .value_name("SLOT_DISTANCE")
                .takes_value(true)
                .default_value("150")
                .help("If --known-validators are specified, report this validator healthy \
                       if its latest account hash is no further behind than this number of \
                       slots from the latest known validator account hash. \
                       If no --known-validators are specified, the validator will always \
                       report itself to be healthy")
        )
        .arg(
            Arg::with_name("rpc_faucet_addr")
                .long("rpc-faucet-address")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Enable the JSON RPC 'requestAirdrop' API with this faucet address."),
        )
        .arg(
            Arg::with_name("account_paths")
                .long("accounts")
                .value_name("PATHS")
                .takes_value(true)
                .multiple(true)
                .help("Comma separated persistent accounts location"),
        )
        .arg(
            Arg::with_name("account_shrink_path")
                .long("account-shrink-path")
                .value_name("PATH")
                .takes_value(true)
                .multiple(true)
                .help("Path to accounts shrink path which can hold a compacted account set."),
        )
        .arg(
            Arg::with_name("snapshots")
                .long("snapshots")
                .value_name("DIR")
                .takes_value(true)
                .help("Use DIR as snapshot location [default: --ledger value]"),
        )
        .arg(
            Arg::with_name("incremental_snapshot_archive_path")
                .long("incremental-snapshot-archive-path")
                .conflicts_with("no-incremental-snapshots")
                .value_name("DIR")
                .takes_value(true)
                .help("Use DIR as separate location for incremental snapshot archives [default: --snapshots value]"),
        )
        .arg(
            Arg::with_name("tower")
                .long("tower")
                .value_name("DIR")
                .takes_value(true)
                .help("Use DIR as file tower storage location [default: --ledger value]"),
        )
        .arg(
            Arg::with_name("tower_storage")
                .long("tower-storage")
                .possible_values(&["file", "etcd"])
                .default_value("file")
                .takes_value(true)
                .help("Where to store the tower"),
        )
        .arg(
            Arg::with_name("etcd_endpoint")
                .long("etcd-endpoint")
                .required_if("tower_storage", "etcd")
                .value_name("HOST:PORT")
                .takes_value(true)
                .multiple(true)
                .validator(solana_net_utils::is_host_port)
                .help("etcd gRPC endpoint to connect with")
        )
        .arg(
            Arg::with_name("etcd_domain_name")
                .long("etcd-domain-name")
                .required_if("tower_storage", "etcd")
                .value_name("DOMAIN")
                .default_value("localhost")
                .takes_value(true)
                .help("domain name against which to verify the etcd server’s TLS certificate")
        )
        .arg(
            Arg::with_name("etcd_cacert_file")
                .long("etcd-cacert-file")
                .required_if("tower_storage", "etcd")
                .value_name("FILE")
                .takes_value(true)
                .help("verify the TLS certificate of the etcd endpoint using this CA bundle")
        )
        .arg(
            Arg::with_name("etcd_key_file")
                .long("etcd-key-file")
                .required_if("tower_storage", "etcd")
                .value_name("FILE")
                .takes_value(true)
                .help("TLS key file to use when establishing a connection to the etcd endpoint")
        )
        .arg(
            Arg::with_name("etcd_cert_file")
                .long("etcd-cert-file")
                .required_if("tower_storage", "etcd")
                .value_name("FILE")
                .takes_value(true)
                .help("TLS certificate to use when establishing a connection to the etcd endpoint")
        )
        .arg(
            Arg::with_name("gossip_port")
                .long("gossip-port")
                .value_name("PORT")
                .takes_value(true)
                .help("Gossip port number for the validator"),
        )
        .arg(
            Arg::with_name("gossip_host")
                .long("gossip-host")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .help("Gossip DNS name or IP address for the validator to advertise in gossip \
                       [default: ask --entrypoint, or 127.0.0.1 when --entrypoint is not provided]"),
        )
        .arg(
            Arg::with_name("tpu_host_addr")
                .long("tpu-host-addr")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Specify TPU address to advertise in gossip [default: ask --entrypoint or localhost\
                    when --entrypoint is not provided]"),

        )
        .arg(
            Arg::with_name("public_rpc_addr")
                .long("public-rpc-address")
                .value_name("HOST:PORT")
                .takes_value(true)
                .conflicts_with("private_rpc")
                .validator(solana_net_utils::is_host_port)
                .help("RPC address for the validator to advertise publicly in gossip. \
                      Useful for validators running behind a load balancer or proxy \
                      [default: use --rpc-bind-address / --rpc-port]"),
        )
        .arg(
            Arg::with_name("dynamic_port_range")
                .long("dynamic-port-range")
                .value_name("MIN_PORT-MAX_PORT")
                .takes_value(true)
                .default_value(default_dynamic_port_range)
                .validator(solana_validator::port_range_validator)
                .help("Range to use for dynamically assigned ports"),
        )
        .arg(
            Arg::with_name("maximum_local_snapshot_age")
                .long("maximum-local-snapshot-age")
                .value_name("NUMBER_OF_SLOTS")
                .takes_value(true)
                .default_value("2500")
                .help("Reuse a local snapshot if it's less than this many \
                       slots behind the highest snapshot available for \
                       download from other validators"),
        )
        .arg(
            Arg::with_name("incremental_snapshots")
                .long("incremental-snapshots")
                .takes_value(false)
                .hidden(true)
                .conflicts_with("no_incremental_snapshots")
                .help("Enable incremental snapshots")
                .long_help("Enable incremental snapshots by setting this flag. \
                   When enabled, --snapshot-interval-slots will set the \
                   incremental snapshot interval. To set the full snapshot \
                   interval, use --full-snapshot-interval-slots.")
         )
        .arg(
            Arg::with_name("no_incremental_snapshots")
                .long("no-incremental-snapshots")
                .takes_value(false)
                .help("Disable incremental snapshots")
                .long_help("Disable incremental snapshots by setting this flag. \
                   When enabled, --snapshot-interval-slots will set the \
                   incremental snapshot interval. To set the full snapshot \
                   interval, use --full-snapshot-interval-slots.")
         )
        .arg(
            Arg::with_name("incremental_snapshot_interval_slots")
                .long("incremental-snapshot-interval-slots")
                .alias("snapshot-interval-slots")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_incremental_snapshot_archive_interval_slots)
                .help("Number of slots between generating snapshots, \
                      0 to disable snapshots"),
        )
        .arg(
            Arg::with_name("full_snapshot_interval_slots")
                .long("full-snapshot-interval-slots")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_full_snapshot_archive_interval_slots)
                .help("Number of slots between generating full snapshots")
        )
        .arg(
            Arg::with_name("maximum_full_snapshots_to_retain")
                .long("maximum-full-snapshots-to-retain")
                .alias("maximum-snapshots-to-retain")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_maximum_full_snapshot_archives_to_retain)
                .help("The maximum number of full snapshot archives to hold on to when purging older snapshots.")
        )
        .arg(
            Arg::with_name("maximum_incremental_snapshots_to_retain")
                .long("maximum-incremental-snapshots-to-retain")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_maximum_incremental_snapshot_archives_to_retain)
                .help("The maximum number of incremental snapshot archives to hold on to when purging older snapshots.")
        )
        .arg(
            Arg::with_name("snapshot_packager_niceness_adj")
                .long("snapshot-packager-niceness-adjustment")
                .value_name("ADJUSTMENT")
                .takes_value(true)
                .validator(is_niceness_adjustment_valid)
                .default_value("0")
                .help("Add this value to niceness of snapshot packager thread. Negative value \
                      increases priority, positive value decreases priority.")
        )
        .arg(
            Arg::with_name("minimal_snapshot_download_speed")
                .long("minimal-snapshot-download-speed")
                .value_name("MINIMAL_SNAPSHOT_DOWNLOAD_SPEED")
                .takes_value(true)
                .default_value(default_min_snapshot_download_speed)
                .help("The minimal speed of snapshot downloads measured in bytes/second. \
                      If the initial download speed falls below this threshold, the system will \
                      retry the download against a different rpc node."),
        )
        .arg(
            Arg::with_name("maximum_snapshot_download_abort")
                .long("maximum-snapshot-download-abort")
                .value_name("MAXIMUM_SNAPSHOT_DOWNLOAD_ABORT")
                .takes_value(true)
                .default_value(default_max_snapshot_download_abort)
                .help("The maximum number of times to abort and retry when encountering a \
                      slow snapshot download."),
        )
        .arg(
            Arg::with_name("contact_debug_interval")
                .long("contact-debug-interval")
                .value_name("CONTACT_DEBUG_INTERVAL")
                .takes_value(true)
                .default_value("120000")
                .help("Milliseconds between printing contact debug from gossip."),
        )
        .arg(
            Arg::with_name("no_poh_speed_test")
                .long("no-poh-speed-test")
                .help("Skip the check for PoH speed."),
        )
        .arg(
            Arg::with_name("no_os_network_limits_test")
                .hidden(true)
                .long("no-os-network-limits-test")
                .help("Skip checks for OS network limits.")
        )
        .arg(
            Arg::with_name("no_os_memory_stats_reporting")
                .long("no-os-memory-stats-reporting")
                .help("Disable reporting of OS memory statistics.")
        )
        .arg(
            Arg::with_name("no_os_network_stats_reporting")
                .long("no-os-network-stats-reporting")
                .help("Disable reporting of OS network statistics.")
        )
        .arg(
            Arg::with_name("no_os_cpu_stats_reporting")
                .long("no-os-cpu-stats-reporting")
                .help("Disable reporting of OS CPU statistics.")
        )
        .arg(
            Arg::with_name("no_os_disk_stats_reporting")
                .long("no-os-disk-stats-reporting")
                .help("Disable reporting of OS disk statistics.")
        )
        .arg(
            Arg::with_name("accounts-hash-interval-slots")
                .long("accounts-hash-interval-slots")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value("100")
                .help("Number of slots between generating accounts hash.")
                .validator(|val| {
                    if val.eq("0") {
                        Err(String::from("Accounts hash interval cannot be zero"))
                    }
                    else {
                        Ok(())
                    }
                }),
        )
        .arg(
            Arg::with_name("snapshot_version")
                .long("snapshot-version")
                .value_name("SNAPSHOT_VERSION")
                .validator(is_parsable::<SnapshotVersion>)
                .takes_value(true)
                .default_value(SnapshotVersion::default().into())
                .help("Output snapshot version"),
        )
        .arg(
            Arg::with_name("limit_ledger_size")
                .long("limit-ledger-size")
                .value_name("SHRED_COUNT")
                .takes_value(true)
                .min_values(0)
                .max_values(1)
                /* .default_value() intentionally not used here! */
                .help("Keep this amount of shreds in root slots."),
        )
        .arg(
            Arg::with_name("rocksdb_shred_compaction")
                .hidden(true)
                .long("rocksdb-shred-compaction")
                .value_name("ROCKSDB_COMPACTION_STYLE")
                .takes_value(true)
                .possible_values(&["level", "fifo"])
                .default_value("level")
                .help("EXPERIMENTAL: Controls how RocksDB compacts shreds. \
                       *WARNING*: You will lose your ledger data when you switch between options. \
                       Possible values are: \
                       'level': stores shreds using RocksDB's default (level) compaction. \
                       'fifo': stores shreds under RocksDB's FIFO compaction. \
                           This option is more efficient on disk-write-bytes of the ledger store."),
        )
        .arg(
            Arg::with_name("rocksdb_fifo_shred_storage_size")
                .hidden(true)
                .long("rocksdb-fifo-shred-storage-size")
                .value_name("SHRED_STORAGE_SIZE_BYTES")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .default_value(default_rocksdb_fifo_shred_storage_size)
                .help("The shred storage size in bytes. \
                       The suggested value is 50% of your ledger storage size in bytes."),
        )
        .arg(
            Arg::with_name("rocksdb_ledger_compression")
                .hidden(true)
                .long("rocksdb-ledger-compression")
                .value_name("COMPRESSION_TYPE")
                .takes_value(true)
                .possible_values(&["none", "lz4", "snappy", "zlib"])
                .default_value("none")
                .help("The compression alrogithm that is used to compress \
                       transaction status data.  \
                       Turning on compression can save ~10% of the ledger size."),
        )
        .arg(
            Arg::with_name("rocksdb_perf_sample_interval")
                .hidden(true)
                .long("rocksdb-perf-sample-interval")
                .value_name("ROCKS_PERF_SAMPLE_INTERVAL")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value("0")
                .help("Controls how often RocksDB read/write performance sample is collected. \
                       Reads/writes perf samples are collected in 1 / ROCKS_PERF_SAMPLE_INTERVAL sampling rate."),

        )
        .arg(
            Arg::with_name("skip_poh_verify")
                .long("skip-poh-verify")
                .takes_value(false)
                .help("Skip ledger verification at validator bootup"),
        )
        .arg(
            Arg::with_name("cuda")
                .long("cuda")
                .takes_value(false)
                .help("Use CUDA"),
        )
        .arg(
            clap::Arg::with_name("require_tower")
                .long("require-tower")
                .takes_value(false)
                .help("Refuse to start if saved tower state is not found"),
        )
        .arg(
            Arg::with_name("expected_genesis_hash")
                .long("expected-genesis-hash")
                .value_name("HASH")
                .takes_value(true)
                .validator(hash_validator)
                .help("Require the genesis have this hash"),
        )
        .arg(
            Arg::with_name("expected_bank_hash")
                .long("expected-bank-hash")
                .value_name("HASH")
                .takes_value(true)
                .validator(hash_validator)
                .help("When wait-for-supermajority <x>, require the bank at <x> to have this hash"),
        )
        .arg(
            Arg::with_name("expected_shred_version")
                .long("expected-shred-version")
                .value_name("VERSION")
                .takes_value(true)
                .validator(is_parsable::<u16>)
                .help("Require the shred version be this value"),
        )
        .arg(
            Arg::with_name("logfile")
                .short("o")
                .long("log")
                .value_name("FILE")
                .takes_value(true)
                .help("Redirect logging to the specified file, '-' for standard error. \
                       Sending the SIGUSR1 signal to the validator process will cause it \
                       to re-open the log file"),
        )
        .arg(
            Arg::with_name("wait_for_supermajority")
                .long("wait-for-supermajority")
                .requires("expected_bank_hash")
                .value_name("SLOT")
                .validator(is_slot)
                .help("After processing the ledger and the next slot is SLOT, wait until a \
                       supermajority of stake is visible on gossip before starting PoH"),
        )
        .arg(
            Arg::with_name("no_wait_for_vote_to_start_leader")
                .hidden(true)
                .long("no-wait-for-vote-to-start-leader")
                .help("If the validator starts up with no ledger, it will wait to start block
                      production until it sees a vote land in a rooted slot. This prevents
                      double signing. Turn off to risk double signing a block."),
        )
        .arg(
            Arg::with_name("hard_forks")
                .long("hard-fork")
                .value_name("SLOT")
                .validator(is_slot)
                .multiple(true)
                .takes_value(true)
                .help("Add a hard fork at this slot"),
        )
        .arg(
            Arg::with_name("known_validators")
                .alias("trusted-validator")
                .long("known-validator")
                .validator(is_pubkey)
                .value_name("VALIDATOR IDENTITY")
                .multiple(true)
                .takes_value(true)
                .help("A snapshot hash must be published in gossip by this validator to be accepted. \
                       May be specified multiple times. If unspecified any snapshot hash will be accepted"),
        )
        .arg(
            Arg::with_name("debug_key")
                .long("debug-key")
                .validator(is_pubkey)
                .value_name("ADDRESS")
                .multiple(true)
                .takes_value(true)
                .help("Log when transactions are processed which reference a given key."),
        )
        .arg(
            Arg::with_name("only_known_rpc")
                .alias("no-untrusted-rpc")
                .long("only-known-rpc")
                .takes_value(false)
                .requires("known_validators")
                .help("Use the RPC service of known validators only")
        )
        .arg(
            Arg::with_name("repair_validators")
                .long("repair-validator")
                .validator(is_pubkey)
                .value_name("VALIDATOR IDENTITY")
                .multiple(true)
                .takes_value(true)
                .help("A list of validators to request repairs from. If specified, repair will not \
                       request from validators outside this set [default: all validators]")
        )
        .arg(
            Arg::with_name("gossip_validators")
                .long("gossip-validator")
                .validator(is_pubkey)
                .value_name("VALIDATOR IDENTITY")
                .multiple(true)
                .takes_value(true)
                .help("A list of validators to gossip with.  If specified, gossip \
                      will not push/pull from from validators outside this set. \
                      [default: all validators]")
        )
        .arg(
            Arg::with_name("no_rocksdb_compaction")
                .long("no-rocksdb-compaction")
                .takes_value(false)
                .help("Disable manual compaction of the ledger database (this is ignored).")
        )
        .arg(
            Arg::with_name("rocksdb_compaction_interval")
                .long("rocksdb-compaction-interval-slots")
                .value_name("ROCKSDB_COMPACTION_INTERVAL_SLOTS")
                .takes_value(true)
                .help("Number of slots between compacting ledger"),
        )
        .arg(
            Arg::with_name("tpu_coalesce_ms")
                .long("tpu-coalesce-ms")
                .value_name("MILLISECS")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .help("Milliseconds to wait in the TPU receiver for packet coalescing."),
        )
        .arg(
            Arg::with_name("tpu_use_quic")
                .long("tpu-use-quic")
                .takes_value(false)
                .hidden(true)
                .conflicts_with("tpu_disable_quic")
                .help("Use QUIC to send transactions."),
        )
        .arg(
            Arg::with_name("tpu_disable_quic")
                .long("tpu-disable-quic")
                .takes_value(false)
                .help("Do not use QUIC to send transactions."),
        )
        .arg(
            Arg::with_name("tpu_enable_udp")
                .long("tpu-enable-udp")
                .takes_value(false)
                .help("Enable UDP for receiving/sending transactions."),
        )
        .arg(
            Arg::with_name("disable_quic_servers")
                .long("disable-quic-servers")
                .takes_value(false)
                .hidden(true)
        )
        .arg(
            Arg::with_name("enable_quic_servers")
                .hidden(true)
                .long("enable-quic-servers")
        )
        .arg(
            Arg::with_name("tpu_connection_pool_size")
                .long("tpu-connection-pool-size")
                .takes_value(true)
                .default_value(default_tpu_connection_pool_size)
                .validator(is_parsable::<usize>)
                .help("Controls the TPU connection pool size per remote address"),
        )
        .arg(
            Arg::with_name("staked_nodes_overrides")
                .long("staked-nodes-overrides")
                .value_name("PATH")
                .takes_value(true)
                .help("Provide path to a yaml file with custom overrides for stakes of specific
                            identities. Overriding the amount of stake this validator considers
                            as valid for other peers in network. The stake amount is used for calculating
                            number of QUIC streams permitted from the peer and vote packet sender stage.
                            Format of the file: `staked_map_id: {<pubkey>: <SOL stake amount>}"),
        )
        .arg(
            Arg::with_name("rocksdb_max_compaction_jitter")
                .long("rocksdb-max-compaction-jitter-slots")
                .value_name("ROCKSDB_MAX_COMPACTION_JITTER_SLOTS")
                .takes_value(true)
                .help("Introduce jitter into the compaction to offset compaction operation"),
        )
        .arg(
            Arg::with_name("bind_address")
                .long("bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .default_value("0.0.0.0")
                .help("IP address to bind the validator ports"),
        )
        .arg(
            Arg::with_name("rpc_bind_address")
                .long("rpc-bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .help("IP address to bind the RPC port [default: 127.0.0.1 if --private-rpc is present, otherwise use --bind-address]"),
        )
        .arg(
            Arg::with_name("rpc_threads")
                .long("rpc-threads")
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .default_value(&default_rpc_threads)
                .help("Number of threads to use for servicing RPC requests"),
        )
        .arg(
            Arg::with_name("rpc_niceness_adj")
                .long("rpc-niceness-adjustment")
                .value_name("ADJUSTMENT")
                .takes_value(true)
                .validator(is_niceness_adjustment_valid)
                .default_value("0")
                .help("Add this value to niceness of RPC threads. Negative value \
                      increases priority, positive value decreases priority.")
        )
        .arg(
            Arg::with_name("rpc_bigtable_timeout")
                .long("rpc-bigtable-timeout")
                .value_name("SECONDS")
                .validator(is_parsable::<u64>)
                .takes_value(true)
                .default_value("30")
                .help("Number of seconds before timing out RPC requests backed by BigTable"),
        )
        .arg(
            Arg::with_name("rpc_bigtable_instance_name")
                .long("rpc-bigtable-instance-name")
                .takes_value(true)
                .value_name("INSTANCE_NAME")
                .default_value(solana_storage_bigtable::DEFAULT_INSTANCE_NAME)
                .help("Name of the Bigtable instance to upload to")
        )
        .arg(
            Arg::with_name("rpc_bigtable_app_profile_id")
                .long("rpc-bigtable-app-profile-id")
                .takes_value(true)
                .value_name("APP_PROFILE_ID")
                .default_value(solana_storage_bigtable::DEFAULT_APP_PROFILE_ID)
                .help("Bigtable application profile id to use in requests")
        )
        .arg(
            Arg::with_name("rpc_pubsub_worker_threads")
                .long("rpc-pubsub-worker-threads")
                .takes_value(true)
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .default_value("4")
                .help("PubSub worker threads"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_enable_block_subscription")
                .long("rpc-pubsub-enable-block-subscription")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .help("Enable the unstable RPC PubSub `blockSubscribe` subscription"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_enable_vote_subscription")
                .long("rpc-pubsub-enable-vote-subscription")
                .takes_value(false)
                .help("Enable the unstable RPC PubSub `voteSubscribe` subscription"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_connections")
                .long("rpc-pubsub-max-connections")
                .value_name("NUMBER")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .hidden(true)
                .help("The maximum number of connections that RPC PubSub will support. \
                       This is a hard limit and no new connections beyond this limit can \
                       be made until an old connection is dropped. (Obsolete)"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_fragment_size")
                .long("rpc-pubsub-max-fragment-size")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .hidden(true)
                .help("The maximum length in bytes of acceptable incoming frames. Messages longer \
                       than this will be rejected. (Obsolete)"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_in_buffer_capacity")
                .long("rpc-pubsub-max-in-buffer-capacity")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .hidden(true)
                .help("The maximum size in bytes to which the incoming websocket buffer can grow. \
                      (Obsolete)"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_out_buffer_capacity")
                .long("rpc-pubsub-max-out-buffer-capacity")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .hidden(true)
                .help("The maximum size in bytes to which the outgoing websocket buffer can grow. \
                       (Obsolete)"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_active_subscriptions")
                .long("rpc-pubsub-max-active-subscriptions")
                .takes_value(true)
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_pubsub_max_active_subscriptions)
                .help("The maximum number of active subscriptions that RPC PubSub will accept \
                       across all connections."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_queue_capacity_items")
                .long("rpc-pubsub-queue-capacity-items")
                .takes_value(true)
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_pubsub_queue_capacity_items)
                .help("The maximum number of notifications that RPC PubSub will store \
                       across all connections."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_queue_capacity_bytes")
                .long("rpc-pubsub-queue-capacity-bytes")
                .takes_value(true)
                .value_name("BYTES")
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_pubsub_queue_capacity_bytes)
                .help("The maximum total size of notifications that RPC PubSub will store \
                       across all connections."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_notification_threads")
                .long("rpc-pubsub-notification-threads")
                .requires("full_rpc_api")
                .takes_value(true)
                .value_name("NUM_THREADS")
                .validator(is_parsable::<usize>)
                .help("The maximum number of threads that RPC PubSub will use \
                       for generating notifications. 0 will disable RPC PubSub notifications"),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_retry_ms")
                .long("rpc-send-retry-ms")
                .value_name("MILLISECS")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .default_value(&default_rpc_send_transaction_retry_ms)
                .help("The rate at which transactions sent via rpc service are retried."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_batch_ms")
                .long("rpc-send-batch-ms")
                .value_name("MILLISECS")
                .hidden(true)
                .takes_value(true)
                .validator(|s| is_within_range(s, 1, MAX_BATCH_SEND_RATE_MS))
                .default_value(&default_rpc_send_transaction_batch_ms)
                .help("The rate at which transactions sent via rpc service are sent in batch."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_leader_forward_count")
                .long("rpc-send-leader-count")
                .value_name("NUMBER")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .default_value(&default_rpc_send_transaction_leader_forward_count)
                .help("The number of upcoming leaders to which to forward transactions sent via rpc service."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_default_max_retries")
                .long("rpc-send-default-max-retries")
                .value_name("NUMBER")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .help("The maximum number of transaction broadcast retries when unspecified by the request, otherwise retried until expiration."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_service_max_retries")
                .long("rpc-send-service-max-retries")
                .value_name("NUMBER")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_send_transaction_service_max_retries)
                .help("The maximum number of transaction broadcast retries, regardless of requested value."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_batch_size")
                .long("rpc-send-batch-size")
                .value_name("NUMBER")
                .hidden(true)
                .takes_value(true)
                .validator(|s| is_within_range(s, 1, MAX_TRANSACTION_BATCH_SIZE))
                .default_value(&default_rpc_send_transaction_batch_size)
                .help("The size of transactions to be sent in batch."),
        )
        .arg(
            Arg::with_name("rpc_scan_and_fix_roots")
                .long("rpc-scan-and-fix-roots")
                .takes_value(false)
                .requires("enable_rpc_transaction_history")
                .help("Verifies blockstore roots on boot and fixes any gaps"),
        )
        .arg(
            Arg::with_name("rpc_max_request_body_size")
                .long("rpc-max-request-body-size")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value(default_rpc_max_request_body_size)
                .help("The maximum request body size accepted by rpc service"),
        )
        .arg(
            Arg::with_name("enable_accountsdb_repl")
                .long("enable-accountsdb-repl")
                .takes_value(false)
                .hidden(true)
                .help("Enable AccountsDb Replication"),
        )
        .arg(
            Arg::with_name("accountsdb_repl_bind_address")
                .long("accountsdb-repl-bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .hidden(true)
                .help("IP address to bind the AccountsDb Replication port [default: use --bind-address]"),
        )
        .arg(
            Arg::with_name("accountsdb_repl_port")
                .long("accountsdb-repl-port")
                .value_name("PORT")
                .takes_value(true)
                .validator(solana_validator::port_validator)
                .hidden(true)
                .help("Enable AccountsDb Replication Service on this port"),
        )
        .arg(
            Arg::with_name("accountsdb_repl_threads")
                .long("accountsdb-repl-threads")
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .default_value(&default_accountsdb_repl_threads)
                .hidden(true)
                .help("Number of threads to use for servicing AccountsDb Replication requests"),
        )
        .arg(
            Arg::with_name("geyser_plugin_config")
                .long("geyser-plugin-config")
                .alias("accountsdb-plugin-config")
                .value_name("FILE")
                .takes_value(true)
                .multiple(true)
                .help("Specify the configuration file for the Geyser plugin."),
        )
        .arg(
            Arg::with_name("halt_on_known_validators_accounts_hash_mismatch")
                .alias("halt-on-trusted-validators-accounts-hash-mismatch")
                .long("halt-on-known-validators-accounts-hash-mismatch")
                .requires("known_validators")
                .takes_value(false)
                .help("Abort the validator if a bank hash mismatch is detected within known validator set"),
        )
        .arg(
            Arg::with_name("snapshot_archive_format")
                .long("snapshot-archive-format")
                .alias("snapshot-compression") // Legacy name used by Solana v1.5.x and older
                .possible_values(SUPPORTED_ARCHIVE_COMPRESSION)
                .default_value(DEFAULT_ARCHIVE_COMPRESSION)
                .value_name("ARCHIVE_TYPE")
                .takes_value(true)
                .help("Snapshot archive format to use."),
        )
        .arg(
            Arg::with_name("max_genesis_archive_unpacked_size")
                .long("max-genesis-archive-unpacked-size")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_genesis_archive_unpacked_size)
                .help(
                    "maximum total uncompressed file size of downloaded genesis archive",
                ),
        )
        .arg(
            Arg::with_name("wal_recovery_mode")
                .long("wal-recovery-mode")
                .value_name("MODE")
                .takes_value(true)
                .possible_values(&[
                    "tolerate_corrupted_tail_records",
                    "absolute_consistency",
                    "point_in_time",
                    "skip_any_corrupted_record"])
                .help(
                    "Mode to recovery the ledger db write ahead log."
                ),
        )
        .arg(
            Arg::with_name("no_bpf_jit")
                .long("no-bpf-jit")
                .takes_value(false)
                .help("Disable the just-in-time compiler and instead use the interpreter for BPF"),
        )
        .arg(
            // legacy nop argument
            Arg::with_name("bpf_jit")
                .long("bpf-jit")
                .hidden(true)
                .takes_value(false)
                .conflicts_with("no_bpf_jit")
        )
        .arg(
            Arg::with_name("poh_pinned_cpu_core")
                .hidden(true)
                .long("experimental-poh-pinned-cpu-core")
                .takes_value(true)
                .value_name("CPU_CORE_INDEX")
                .validator(|s| {
                    let core_index = usize::from_str(&s).map_err(|e| e.to_string())?;
                    let max_index = core_affinity::get_core_ids().map(|cids| cids.len() - 1).unwrap_or(0);
                    if core_index > max_index {
                        return Err(format!("core index must be in the range [0, {}]", max_index));
                    }
                    Ok(())
                })
                .help("EXPERIMENTAL: Specify which CPU core PoH is pinned to"),
        )
        .arg(
            Arg::with_name("poh_hashes_per_batch")
                .hidden(true)
                .long("poh-hashes-per-batch")
                .takes_value(true)
                .value_name("NUM")
                .help("Specify hashes per batch in PoH service"),
        )
        .arg(
            Arg::with_name("account_indexes")
                .long("account-index")
                .takes_value(true)
                .multiple(true)
                .possible_values(&["program-id", "spl-token-owner", "spl-token-mint"])
                .value_name("INDEX")
                .help("Enable an accounts index, indexed by the selected account field"),
        )
        .arg(
            Arg::with_name("account_index_exclude_key")
                .long(EXCLUDE_KEY)
                .takes_value(true)
                .validator(is_pubkey)
                .multiple(true)
                .value_name("KEY")
                .help("When account indexes are enabled, exclude this key from the index."),
        )
        .arg(
            Arg::with_name("account_index_include_key")
                .long(INCLUDE_KEY)
                .takes_value(true)
                .validator(is_pubkey)
                .conflicts_with("account_index_exclude_key")
                .multiple(true)
                .value_name("KEY")
                .help("When account indexes are enabled, only include specific keys in the index. This overrides --account-index-exclude-key."),
        )
        .arg(
            Arg::with_name("no_accounts_db_caching")
                .long("no-accounts-db-caching")
                .help("Disables accounts caching"),
        )
        .arg(
            Arg::with_name("accounts_db_verify_refcounts")
                .long("accounts-db-verify-refcounts")
                .help("Debug option to scan all append vecs and verify account index refcounts prior to clean")
                .hidden(true)
        )
        .arg(
            Arg::with_name("accounts_db_skip_shrink")
                .long("accounts-db-skip-shrink")
                .help("Enables faster starting of validators by skipping shrink. \
                      This option is for use during testing."),
        )
        .arg(
            Arg::with_name("accounts_db_skip_rewrites")
                .long("accounts-db-skip-rewrites")
                .help("Accounts that are rent exempt and have no changes are not rewritten. \
                      This produces snapshots that older versions cannot read.")
                      .hidden(true),
        )
        .arg(
            Arg::with_name("accounts_db_ancient_append_vecs")
                .long("accounts-db-ancient-append-vecs")
                .help("AppendVecs that are older than an epoch are squashed together.")
                      .hidden(true),
        )
        .arg(
            Arg::with_name("accounts_db_cache_limit_mb")
                .long("accounts-db-cache-limit-mb")
                .value_name("MEGABYTES")
                .validator(is_parsable::<u64>)
                .takes_value(true)
                .help("How large the write cache for account data can become. If this is exceeded, the cache is flushed more aggressively."),
        )
        .arg(
            Arg::with_name("accounts_index_scan_results_limit_mb")
                .long("accounts-index-scan-results-limit-mb")
                .value_name("MEGABYTES")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .help("How large accumulated results from an accounts index scan can become. If this is exceeded, the scan aborts."),
        )
        .arg(
            Arg::with_name("accounts_index_memory_limit_mb")
                .long("accounts-index-memory-limit-mb")
                .value_name("MEGABYTES")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .help("How much memory the accounts index can consume. If this is exceeded, some account index entries will be stored on disk."),
        )
        .arg(
            Arg::with_name("disable_accounts_disk_index")
                .long("disable-accounts-disk-index")
                .help("Disable the disk-based accounts index if it is enabled by default.")
                .conflicts_with("accounts_index_memory_limit_mb")
        )
        .arg(
            Arg::with_name("accounts_index_bins")
                .long("accounts-index-bins")
                .value_name("BINS")
                .validator(is_pow2)
                .takes_value(true)
                .help("Number of bins to divide the accounts index into"),
        )
        .arg(
            Arg::with_name("accounts_hash_num_passes")
                .long("accounts-hash-num-passes")
                .value_name("PASSES")
                .validator(is_pow2)
                .takes_value(true)
                .help("Number of passes to calculate the hash of all accounts"),
        )
        .arg(
            Arg::with_name("accounts_index_path")
                .long("accounts-index-path")
                .value_name("PATH")
                .takes_value(true)
                .multiple(true)
                .help("Persistent accounts-index location. \
                       May be specified multiple times. \
                       [default: [ledger]/accounts_index]"),
         )
         .arg(Arg::with_name("accounts_filler_count")
            .long("accounts-filler-count")
            .value_name("COUNT")
            .validator(is_parsable::<usize>)
            .takes_value(true)
            .default_value("0")
            .help("How many accounts to add to stress the system. Accounts are ignored in operations related to correctness."))
         .arg(Arg::with_name("accounts_filler_size")
            .long("accounts-filler-size")
            .value_name("BYTES")
            .validator(is_parsable::<usize>)
            .takes_value(true)
            .default_value("0")
            .requires("accounts_filler_count")
            .help("Size per filler account in bytes."))
         .arg(
            Arg::with_name("accounts_db_test_hash_calculation")
                .long("accounts-db-test-hash-calculation")
                .help("Enables testing of hash calculation using stores in \
                      AccountsHashVerifier. This has a computational cost."),
        )
        .arg(
            Arg::with_name("accounts_db_index_hashing")
                .long("accounts-db-index-hashing")
                .help("Enables the use of the index in hash calculation in \
                       AccountsHashVerifier/Accounts Background Service.")
                .hidden(true),
        )
        .arg(
            Arg::with_name("no_accounts_db_index_hashing")
                .long("no-accounts-db-index-hashing")
                .help("This is obsolete. See --accounts-db-index-hashing. \
                       Disables the use of the index in hash calculation in \
                       AccountsHashVerifier/Accounts Background Service.")
                .hidden(true),
        )
        .arg(
            // legacy nop argument
            Arg::with_name("accounts_db_caching_enabled")
                .long("accounts-db-caching-enabled")
                .conflicts_with("no_accounts_db_caching")
                .hidden(true)
        )
        .arg(
            Arg::with_name("accounts_shrink_optimize_total_space")
                .long("accounts-shrink-optimize-total-space")
                .takes_value(true)
                .value_name("BOOLEAN")
                .default_value(default_accounts_shrink_optimize_total_space)
                .help("When this is set to true, the system will shrink the most \
                       sparse accounts and when the overall shrink ratio is above \
                       the specified accounts-shrink-ratio, the shrink will stop and \
                       it will skip all other less sparse accounts."),
        )
        .arg(
            Arg::with_name("accounts_shrink_ratio")
                .long("accounts-shrink-ratio")
                .takes_value(true)
                .value_name("RATIO")
                .default_value(default_accounts_shrink_ratio)
                .help("Specifies the shrink ratio for the accounts to be shrunk. \
                       The shrink ratio is defined as the ratio of the bytes alive over the  \
                       total bytes used. If the account's shrink ratio is less than this ratio \
                       it becomes a candidate for shrinking. The value must between 0. and 1.0 \
                       inclusive."),
        )
        .arg(
            Arg::with_name("no_duplicate_instance_check")
                .long("no-duplicate-instance-check")
                .takes_value(false)
                .help("Disables duplicate instance check")
                .hidden(true),
        )
        .arg(
            Arg::with_name("allow_private_addr")
                .long("allow-private-addr")
                .takes_value(false)
                .help("Allow contacting private ip addresses")
                .hidden(true),
        )
        .arg(
            Arg::with_name("log_messages_bytes_limit")
                .long("log-messages-bytes-limit")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .value_name("BYTES")
                .help("Maximum number of bytes written to the program log before truncation")
        )
        .arg(
            Arg::with_name("replay_slots_concurrently")
                .long("replay-slots-concurrently")
                .help("Allow concurrent replay of slots on different forks")
        )
        .after_help("The default subcommand is run")
        .subcommand(
            SubCommand::with_name("exit")
            .about("Send an exit request to the validator")
            .arg(
                Arg::with_name("force")
                    .short("f")
                    .long("force")
                    .takes_value(false)
                    .help("Request the validator exit immediately instead of waiting for a restart window")
            )
            .arg(
                Arg::with_name("monitor")
                    .short("m")
                    .long("monitor")
                    .takes_value(false)
                    .help("Monitor the validator after sending the exit request")
            )
            .arg(
                Arg::with_name("min_idle_time")
                    .long("min-idle-time")
                    .takes_value(true)
                    .validator(is_parsable::<usize>)
                    .value_name("MINUTES")
                    .default_value("10")
                    .help("Minimum time that the validator should not be leader before restarting")
            )
            .arg(
                Arg::with_name("max_delinquent_stake")
                    .long("max-delinquent-stake")
                    .takes_value(true)
                    .validator(is_valid_percentage)
                    .default_value("5")
                    .value_name("PERCENT")
                    .help("The maximum delinquent stake % permitted for an exit")
            )
            .arg(
                Arg::with_name("skip_new_snapshot_check")
                    .long("skip-new-snapshot-check")
                    .help("Skip check for a new snapshot")
            )
        )
        .subcommand(
            SubCommand::with_name("authorized-voter")
            .about("Adjust the validator authorized voters")
            .setting(AppSettings::SubcommandRequiredElseHelp)
            .setting(AppSettings::InferSubcommands)
            .subcommand(
                SubCommand::with_name("add")
                .about("Add an authorized voter")
                .arg(
                    Arg::with_name("authorized_voter_keypair")
                        .index(1)
                        .value_name("KEYPAIR")
                        .required(false)
                        .takes_value(true)
                        .validator(is_keypair)
                        .help("Path to keypair of the authorized voter to add \
                               [default: read JSON keypair from stdin]"),
                )
                .after_help("Note: the new authorized voter only applies to the \
                             currently running validator instance")
            )
            .subcommand(
                SubCommand::with_name("remove-all")
                .about("Remove all authorized voters")
                .after_help("Note: the removal only applies to the \
                             currently running validator instance")
            )
        )
        .subcommand(
            SubCommand::with_name("contact-info")
            .about("Display the validator's contact info")
            .arg(
                Arg::with_name("output")
                    .long("output")
                    .takes_value(true)
                    .value_name("MODE")
                    .possible_values(&["json", "json-compact"])
                    .help("Output display mode")
            )
        )
        .subcommand(
            SubCommand::with_name("init")
            .about("Initialize the ledger directory then exit")
        )
        .subcommand(
            SubCommand::with_name("monitor")
            .about("Monitor the validator")
        )
        .subcommand(
            SubCommand::with_name("run")
            .about("Run the validator")
        )
        .subcommand(
            SubCommand::with_name("set-identity")
            .about("Set the validator identity")
            .arg(
                Arg::with_name("identity")
                    .index(1)
                    .value_name("KEYPAIR")
                    .required(false)
                    .takes_value(true)
                    .validator(is_keypair)
                    .help("Path to validator identity keypair \
                           [default: read JSON keypair from stdin]")
            )
            .arg(
                clap::Arg::with_name("require_tower")
                    .long("require-tower")
                    .takes_value(false)
                    .help("Refuse to set the validator identity if saved tower state is not found"),
            )
            .after_help("Note: the new identity only applies to the \
                         currently running validator instance")
        )
        .subcommand(
            SubCommand::with_name("set-log-filter")
            .about("Adjust the validator log filter")
            .arg(
                Arg::with_name("filter")
                    .takes_value(true)
                    .index(1)
                    .help("New filter using the same format as the RUST_LOG environment variable")
            )
            .after_help("Note: the new filter only applies to the currently running validator instance")
        )
        .subcommand(
            SubCommand::with_name("staked-nodes-overrides")
            .about("Overrides stakes of specific node identities.")
            .arg(
                Arg::with_name("path")
                    .value_name("PATH")
                    .takes_value(true)
                    .required(true)
                    .help("Provide path to a file with custom overrides for stakes of specific validator identities."),
            )
            .after_help("Note: the new staked nodes overrides only applies to the \
                         currently running validator instance")
        )
        .subcommand(
            SubCommand::with_name("wait-for-restart-window")
            .about("Monitor the validator for a good time to restart")
            .arg(
                Arg::with_name("min_idle_time")
                    .long("min-idle-time")
                    .takes_value(true)
                    .validator(is_parsable::<usize>)
                    .value_name("MINUTES")
                    .default_value("10")
                    .help("Minimum time that the validator should not be leader before restarting")
            )
            .arg(
                Arg::with_name("identity")
                    .long("identity")
                    .value_name("ADDRESS")
                    .takes_value(true)
                    .validator(is_pubkey_or_keypair)
                    .help("Validator identity to monitor [default: your validator]")
            )
            .arg(
                Arg::with_name("max_delinquent_stake")
                    .long("max-delinquent-stake")
                    .takes_value(true)
                    .validator(is_valid_percentage)
                    .default_value("5")
                    .value_name("PERCENT")
                    .help("The maximum delinquent stake % permitted for a restart")
            )
            .arg(
                Arg::with_name("skip_new_snapshot_check")
                    .long("skip-new-snapshot-check")
                    .help("Skip check for a new snapshot")
            )
            .after_help("Note: If this command exits with a non-zero status \
                         then this not a good time for a restart")
        )
        .get_matches();

    let socket_addr_space = SocketAddrSpace::new(matches.is_present("allow_private_addr"));
    let ledger_path = PathBuf::from(matches.value_of("ledger_path").unwrap());

    let operation = match matches.subcommand() {
        ("", _) | ("run", _) => Operation::Run,
        ("authorized-voter", Some(authorized_voter_subcommand_matches)) => {
            match authorized_voter_subcommand_matches.subcommand() {
                ("add", Some(subcommand_matches)) => {
                    if let Some(authorized_voter_keypair) =
                        value_t!(subcommand_matches, "authorized_voter_keypair", String).ok()
                    {
                        let authorized_voter_keypair = fs::canonicalize(&authorized_voter_keypair)
                            .unwrap_or_else(|err| {
                                println!(
                                    "Unable to access path: {}: {:?}",
                                    authorized_voter_keypair, err
                                );
                                exit(1);
                            });
                        println!(
                            "Adding authorized voter path: {}",
                            authorized_voter_keypair.display()
                        );

                        let admin_client = admin_rpc_service::connect(&ledger_path);
                        admin_rpc_service::runtime()
                            .block_on(async move {
                                admin_client
                                    .await?
                                    .add_authorized_voter(
                                        authorized_voter_keypair.display().to_string(),
                                    )
                                    .await
                            })
                            .unwrap_or_else(|err| {
                                println!("addAuthorizedVoter request failed: {}", err);
                                exit(1);
                            });
                    } else {
                        let mut stdin = std::io::stdin();
                        let authorized_voter_keypair =
                            read_keypair(&mut stdin).unwrap_or_else(|err| {
                                println!("Unable to read JSON keypair from stdin: {:?}", err);
                                exit(1);
                            });
                        println!(
                            "Adding authorized voter: {}",
                            authorized_voter_keypair.pubkey()
                        );

                        let admin_client = admin_rpc_service::connect(&ledger_path);
                        admin_rpc_service::runtime()
                            .block_on(async move {
                                admin_client
                                    .await?
                                    .add_authorized_voter_from_bytes(Vec::from(
                                        authorized_voter_keypair.to_bytes(),
                                    ))
                                    .await
                            })
                            .unwrap_or_else(|err| {
                                println!("addAuthorizedVoterFromBytes request failed: {}", err);
                                exit(1);
                            });
                    }

                    return;
                }
                ("remove-all", _) => {
                    let admin_client = admin_rpc_service::connect(&ledger_path);
                    admin_rpc_service::runtime()
                        .block_on(async move {
                            admin_client.await?.remove_all_authorized_voters().await
                        })
                        .unwrap_or_else(|err| {
                            println!("removeAllAuthorizedVoters request failed: {}", err);
                            exit(1);
                        });
                    println!("All authorized voters removed");
                    return;
                }
                _ => unreachable!(),
            }
        }
        ("contact-info", Some(subcommand_matches)) => {
            let output_mode = subcommand_matches.value_of("output");
            let admin_client = admin_rpc_service::connect(&ledger_path);
            let contact_info = admin_rpc_service::runtime()
                .block_on(async move { admin_client.await?.contact_info().await })
                .unwrap_or_else(|err| {
                    eprintln!("Contact info query failed: {}", err);
                    exit(1);
                });
            if let Some(mode) = output_mode {
                match mode {
                    "json" => println!("{}", serde_json::to_string_pretty(&contact_info).unwrap()),
                    "json-compact" => print!("{}", serde_json::to_string(&contact_info).unwrap()),
                    _ => unreachable!(),
                }
            } else {
                print!("{}", contact_info);
            }
            return;
        }
        ("init", _) => Operation::Initialize,
        ("exit", Some(subcommand_matches)) => {
            let min_idle_time = value_t_or_exit!(subcommand_matches, "min_idle_time", usize);
            let force = subcommand_matches.is_present("force");
            let monitor = subcommand_matches.is_present("monitor");
            let skip_new_snapshot_check = subcommand_matches.is_present("skip_new_snapshot_check");
            let max_delinquent_stake =
                value_t_or_exit!(subcommand_matches, "max_delinquent_stake", u8);

            if !force {
                wait_for_restart_window(
                    &ledger_path,
                    None,
                    min_idle_time,
                    max_delinquent_stake,
                    skip_new_snapshot_check,
                )
                .unwrap_or_else(|err| {
                    println!("{}", err);
                    exit(1);
                });
            }

            let admin_client = admin_rpc_service::connect(&ledger_path);
            admin_rpc_service::runtime()
                .block_on(async move { admin_client.await?.exit().await })
                .unwrap_or_else(|err| {
                    println!("exit request failed: {}", err);
                    exit(1);
                });
            println!("Exit request sent");

            if monitor {
                monitor_validator(&ledger_path);
            }
            return;
        }
        ("monitor", _) => {
            monitor_validator(&ledger_path);
            return;
        }
        ("staked-nodes-overrides", Some(subcommand_matches)) => {
            if !subcommand_matches.is_present("path") {
                println!(
                    "staked-nodes-overrides requires argument of location of the configuration"
                );
                exit(1);
            }

            let path = subcommand_matches.value_of("path").unwrap();

            let admin_client = admin_rpc_service::connect(&ledger_path);
            admin_rpc_service::runtime()
                .block_on(async move {
                    admin_client
                        .await?
                        .set_staked_nodes_overrides(path.to_string())
                        .await
                })
                .unwrap_or_else(|err| {
                    println!("setStakedNodesOverrides request failed: {}", err);
                    exit(1);
                });
            return;
        }
        ("set-identity", Some(subcommand_matches)) => {
            let require_tower = subcommand_matches.is_present("require_tower");

            if let Some(identity_keypair) = value_t!(subcommand_matches, "identity", String).ok() {
                let identity_keypair = fs::canonicalize(&identity_keypair).unwrap_or_else(|err| {
                    println!("Unable to access path: {}: {:?}", identity_keypair, err);
                    exit(1);
                });
                println!(
                    "New validator identity path: {}",
                    identity_keypair.display()
                );

                let admin_client = admin_rpc_service::connect(&ledger_path);
                admin_rpc_service::runtime()
                    .block_on(async move {
                        admin_client
                            .await?
                            .set_identity(identity_keypair.display().to_string(), require_tower)
                            .await
                    })
                    .unwrap_or_else(|err| {
                        println!("setIdentity request failed: {}", err);
                        exit(1);
                    });
            } else {
                let mut stdin = std::io::stdin();
                let identity_keypair = read_keypair(&mut stdin).unwrap_or_else(|err| {
                    println!("Unable to read JSON keypair from stdin: {:?}", err);
                    exit(1);
                });
                println!("New validator identity: {}", identity_keypair.pubkey());

                let admin_client = admin_rpc_service::connect(&ledger_path);
                admin_rpc_service::runtime()
                    .block_on(async move {
                        admin_client
                            .await?
                            .set_identity_from_bytes(
                                Vec::from(identity_keypair.to_bytes()),
                                require_tower,
                            )
                            .await
                    })
                    .unwrap_or_else(|err| {
                        println!("setIdentityFromBytes request failed: {}", err);
                        exit(1);
                    });
            };

            return;
        }
        ("set-log-filter", Some(subcommand_matches)) => {
            let filter = value_t_or_exit!(subcommand_matches, "filter", String);
            let admin_client = admin_rpc_service::connect(&ledger_path);
            admin_rpc_service::runtime()
                .block_on(async move { admin_client.await?.set_log_filter(filter).await })
                .unwrap_or_else(|err| {
                    println!("set log filter failed: {}", err);
                    exit(1);
                });
            return;
        }
        ("wait-for-restart-window", Some(subcommand_matches)) => {
            let min_idle_time = value_t_or_exit!(subcommand_matches, "min_idle_time", usize);
            let identity = pubkey_of(subcommand_matches, "identity");
            let max_delinquent_stake =
                value_t_or_exit!(subcommand_matches, "max_delinquent_stake", u8);
            let skip_new_snapshot_check = subcommand_matches.is_present("skip_new_snapshot_check");

            wait_for_restart_window(
                &ledger_path,
                identity,
                min_idle_time,
                max_delinquent_stake,
                skip_new_snapshot_check,
            )
            .unwrap_or_else(|err| {
                println!("{}", err);
                exit(1);
            });
            return;
        }
        _ => unreachable!(),
    };

    let identity_keypair = keypair_of(&matches, "identity").unwrap_or_else(|| {
        clap::Error::with_description(
            "The --identity <KEYPAIR> argument is required",
            clap::ErrorKind::ArgumentNotFound,
        )
        .exit();
    });

    let logfile = {
        let logfile = matches
            .value_of("logfile")
            .map(|s| s.into())
            .unwrap_or_else(|| format!("solana-validator-{}.log", identity_keypair.pubkey()));

        if logfile == "-" {
            None
        } else {
            println!("log file: {}", logfile);
            Some(logfile)
        }
    };
    let use_progress_bar = logfile.is_none();
    let _logger_thread = redirect_stderr_to_file(logfile);

    info!("{} {}", crate_name!(), solana_version::version!());
    info!("Starting validator with: {:#?}", std::env::args_os());

    let cuda = matches.is_present("cuda");
    if cuda {
        solana_perf::perf_libs::init_cuda();
        enable_recycler_warming();
    }

    solana_core::validator::report_target_features();

    let authorized_voter_keypairs = keypairs_of(&matches, "authorized_voter_keypairs")
        .map(|keypairs| keypairs.into_iter().map(Arc::new).collect())
        .unwrap_or_else(|| {
            vec![Arc::new(
                keypair_of(&matches, "identity").expect("identity"),
            )]
        });
    let authorized_voter_keypairs = Arc::new(RwLock::new(authorized_voter_keypairs));

    let staked_nodes_overrides_path = matches
        .value_of("staked_nodes_overrides")
        .map(str::to_string);
    let staked_nodes_overrides = Arc::new(RwLock::new(
        match staked_nodes_overrides_path {
            None => StakedNodesOverrides::default(),
            Some(p) => load_staked_nodes_overrides(&p).unwrap_or_else(|err| {
                error!("Failed to load stake-nodes-overrides from {}: {}", &p, err);
                clap::Error::with_description(
                    "Failed to load configuration of stake-nodes-overrides argument",
                    clap::ErrorKind::InvalidValue,
                )
                .exit()
            }),
        }
        .staked_map_id,
    ));

    let init_complete_file = matches.value_of("init_complete_file");

    if matches.is_present("no_check_vote_account") {
        info!("vote account sanity checks are no longer performed by default. --no-check-vote-account is deprecated and can be removed from the command line");
    }
    let rpc_bootstrap_config = bootstrap::RpcBootstrapConfig {
        no_genesis_fetch: matches.is_present("no_genesis_fetch"),
        no_snapshot_fetch: matches.is_present("no_snapshot_fetch"),
        check_vote_account: matches
            .value_of("check_vote_account")
            .map(|url| url.to_string()),
        only_known_rpc: matches.is_present("only_known_rpc"),
        max_genesis_archive_unpacked_size: value_t_or_exit!(
            matches,
            "max_genesis_archive_unpacked_size",
            u64
        ),
        incremental_snapshot_fetch: !matches.is_present("no_incremental_snapshots"),
    };

    let private_rpc = matches.is_present("private_rpc");
    let do_port_check = !matches.is_present("no_port_check");
    let no_rocksdb_compaction = true;
    let rocksdb_compaction_interval = value_t!(matches, "rocksdb_compaction_interval", u64).ok();
    let rocksdb_max_compaction_jitter =
        value_t!(matches, "rocksdb_max_compaction_jitter", u64).ok();
    let tpu_coalesce_ms =
        value_t!(matches, "tpu_coalesce_ms", u64).unwrap_or(DEFAULT_TPU_COALESCE_MS);
    let wal_recovery_mode = matches
        .value_of("wal_recovery_mode")
        .map(BlockstoreRecoveryMode::from);

    // Canonicalize ledger path to avoid issues with symlink creation
    let _ = fs::create_dir_all(&ledger_path);
    let ledger_path = fs::canonicalize(&ledger_path).unwrap_or_else(|err| {
        eprintln!("Unable to access ledger path: {:?}", err);
        exit(1);
    });

    let debug_keys: Option<Arc<HashSet<_>>> = if matches.is_present("debug_key") {
        Some(Arc::new(
            values_t_or_exit!(matches, "debug_key", Pubkey)
                .into_iter()
                .collect(),
        ))
    } else {
        None
    };

    let known_validators = validators_set(
        &identity_keypair.pubkey(),
        &matches,
        "known_validators",
        "--known-validator",
    );
    let repair_validators = validators_set(
        &identity_keypair.pubkey(),
        &matches,
        "repair_validators",
        "--repair-validator",
    );
    let gossip_validators = validators_set(
        &identity_keypair.pubkey(),
        &matches,
        "gossip_validators",
        "--gossip-validator",
    );

    let bind_address = solana_net_utils::parse_host(matches.value_of("bind_address").unwrap())
        .expect("invalid bind_address");
    let rpc_bind_address = if matches.is_present("rpc_bind_address") {
        solana_net_utils::parse_host(matches.value_of("rpc_bind_address").unwrap())
            .expect("invalid rpc_bind_address")
    } else if private_rpc {
        solana_net_utils::parse_host("127.0.0.1").unwrap()
    } else {
        bind_address
    };

    let contact_debug_interval = value_t_or_exit!(matches, "contact_debug_interval", u64);

    let account_indexes = process_account_indexes(&matches);

    let restricted_repair_only_mode = matches.is_present("restricted_repair_only_mode");
    let accounts_shrink_optimize_total_space =
        value_t_or_exit!(matches, "accounts_shrink_optimize_total_space", bool);
    let tpu_use_quic = !matches.is_present("tpu_disable_quic");
    let tpu_enable_udp = if matches.is_present("tpu_enable_udp") {
        true
    } else {
        DEFAULT_TPU_ENABLE_UDP
    };

    let tpu_connection_pool_size = value_t_or_exit!(matches, "tpu_connection_pool_size", usize);

    let shrink_ratio = value_t_or_exit!(matches, "accounts_shrink_ratio", f64);
    if !(0.0..=1.0).contains(&shrink_ratio) {
        eprintln!(
            "The specified account-shrink-ratio is invalid, it must be between 0. and 1.0 inclusive: {}",
            shrink_ratio
        );
        exit(1);
    }

    let accounts_shrink_ratio = if accounts_shrink_optimize_total_space {
        AccountShrinkThreshold::TotalSpace { shrink_ratio }
    } else {
        AccountShrinkThreshold::IndividualStore { shrink_ratio }
    };
    let entrypoint_addrs = values_t!(matches, "entrypoint", String)
        .unwrap_or_default()
        .into_iter()
        .map(|entrypoint| {
            solana_net_utils::parse_host_port(&entrypoint).unwrap_or_else(|e| {
                eprintln!("failed to parse entrypoint address: {}", e);
                exit(1);
            })
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for addr in &entrypoint_addrs {
        if !socket_addr_space.check(addr) {
            eprintln!("invalid entrypoint address: {}", addr);
            exit(1);
        }
    }
    // TODO: Once entrypoints are updated to return shred-version, this should
    // abort if it fails to obtain a shred-version, so that nodes always join
    // gossip with a valid shred-version. The code to adopt entrypoint shred
    // version can then be deleted from gossip and get_rpc_node above.
    let expected_shred_version = value_t!(matches, "expected_shred_version", u16)
        .ok()
        .or_else(|| get_cluster_shred_version(&entrypoint_addrs));

    let tower_storage: Arc<dyn solana_core::tower_storage::TowerStorage> =
        match value_t_or_exit!(matches, "tower_storage", String).as_str() {
            "file" => {
                let tower_path = value_t!(matches, "tower", PathBuf)
                    .ok()
                    .unwrap_or_else(|| ledger_path.clone());

                Arc::new(tower_storage::FileTowerStorage::new(tower_path))
            }
            "etcd" => {
                let endpoints = values_t_or_exit!(matches, "etcd_endpoint", String);
                let domain_name = value_t_or_exit!(matches, "etcd_domain_name", String);
                let ca_certificate_file = value_t_or_exit!(matches, "etcd_cacert_file", String);
                let identity_certificate_file = value_t_or_exit!(matches, "etcd_cert_file", String);
                let identity_private_key_file = value_t_or_exit!(matches, "etcd_key_file", String);

                let read = |file| {
                    fs::read(&file).unwrap_or_else(|err| {
                        eprintln!("Unable to read {}: {}", file, err);
                        exit(1)
                    })
                };

                let tls_config = tower_storage::EtcdTlsConfig {
                    domain_name,
                    ca_certificate: read(ca_certificate_file),
                    identity_certificate: read(identity_certificate_file),
                    identity_private_key: read(identity_private_key_file),
                };

                Arc::new(
                    tower_storage::EtcdTowerStorage::new(endpoints, Some(tls_config))
                        .unwrap_or_else(|err| {
                            eprintln!("Failed to connect to etcd: {}", err);
                            exit(1);
                        }),
                )
            }
            _ => unreachable!(),
        };

    let mut accounts_index_config = AccountsIndexConfig {
        started_from_validator: true, // this is the only place this is set
        ..AccountsIndexConfig::default()
    };
    if let Some(bins) = value_t!(matches, "accounts_index_bins", usize).ok() {
        accounts_index_config.bins = Some(bins);
    }

    accounts_index_config.index_limit_mb =
        if let Some(limit) = value_t!(matches, "accounts_index_memory_limit_mb", usize).ok() {
            IndexLimitMb::Limit(limit)
        } else if matches.is_present("disable_accounts_disk_index") {
            IndexLimitMb::InMemOnly
        } else {
            IndexLimitMb::Unspecified
        };

    {
        let mut accounts_index_paths: Vec<PathBuf> = if matches.is_present("accounts_index_path") {
            values_t_or_exit!(matches, "accounts_index_path", String)
                .into_iter()
                .map(PathBuf::from)
                .collect()
        } else {
            vec![]
        };
        if accounts_index_paths.is_empty() {
            accounts_index_paths = vec![ledger_path.join("accounts_index")];
        }
        accounts_index_config.drives = Some(accounts_index_paths);
    }

    const MB: usize = 1_024 * 1_024;
    accounts_index_config.scan_results_limit_bytes =
        value_t!(matches, "accounts_index_scan_results_limit_mb", usize)
            .ok()
            .map(|mb| mb * MB);

    let filler_accounts_config = FillerAccountsConfig {
        count: value_t_or_exit!(matches, "accounts_filler_count", usize),
        size: value_t_or_exit!(matches, "accounts_filler_size", usize),
    };

    let mut accounts_db_config = AccountsDbConfig {
        index: Some(accounts_index_config),
        accounts_hash_cache_path: Some(ledger_path.clone()),
        filler_accounts_config,
        write_cache_limit_bytes: value_t!(matches, "accounts_db_cache_limit_mb", u64)
            .ok()
            .map(|mb| mb * MB as u64),
        skip_rewrites: matches.is_present("accounts_db_skip_rewrites"),
        ancient_append_vecs: matches.is_present("accounts_db_ancient_append_vecs"),
        exhaustively_verify_refcounts: matches.is_present("accounts_db_verify_refcounts"),
        ..AccountsDbConfig::default()
    };

    if let Some(passes) = value_t!(matches, "accounts_hash_num_passes", usize).ok() {
        accounts_db_config.hash_calc_num_passes = Some(passes);
    }
    let accounts_db_config = Some(accounts_db_config);

    let geyser_plugin_config_files = if matches.is_present("geyser_plugin_config") {
        Some(
            values_t_or_exit!(matches, "geyser_plugin_config", String)
                .into_iter()
                .map(PathBuf::from)
                .collect(),
        )
    } else {
        None
    };

    if matches.is_present("minimal_rpc_api") {
        warn!("--minimal-rpc-api is now the default behavior. This flag is deprecated and can be removed from the launch args");
    }

    if matches.is_present("enable_cpi_and_log_storage") {
        warn!(
            "--enable-cpi-and-log-storage is deprecated. Please update the \
            launch args to use --enable-extended-tx-metadata-storage and remove \
            --enable-cpi-and-log-storage"
        );
    }

    if matches.is_present("enable_quic_servers") {
        warn!("--enable-quic-servers is now the default behavior. This flag is deprecated and can be removed from the launch args");
    }
    if matches.is_present("disable_quic_servers") {
        warn!("--disable-quic-servers is deprecated. The quic server cannot be disabled.");
    }

    let rpc_bigtable_config = if matches.is_present("enable_rpc_bigtable_ledger_storage")
        || matches.is_present("enable_bigtable_ledger_upload")
    {
        Some(RpcBigtableConfig {
            enable_bigtable_ledger_upload: matches.is_present("enable_bigtable_ledger_upload"),
            bigtable_instance_name: value_t_or_exit!(matches, "rpc_bigtable_instance_name", String),
            bigtable_app_profile_id: value_t_or_exit!(
                matches,
                "rpc_bigtable_app_profile_id",
                String
            ),
            timeout: value_t!(matches, "rpc_bigtable_timeout", u64)
                .ok()
                .map(Duration::from_secs),
        })
    } else {
        None
    };

    if matches.is_present("accounts_db_index_hashing") {
        info!("The accounts hash is only calculated without using the index. --accounts-db-index-hashing is deprecated and can be removed from the command line");
    }
    if matches.is_present("no_accounts_db_index_hashing") {
        info!("The accounts hash is only calculated without using the index. --no-accounts-db-index-hashing is deprecated and can be removed from the command line");
    }
    let rpc_send_retry_rate_ms = value_t_or_exit!(matches, "rpc_send_transaction_retry_ms", u64);
    let rpc_send_batch_size = value_t_or_exit!(matches, "rpc_send_transaction_batch_size", usize);
    let rpc_send_batch_send_rate_ms =
        value_t_or_exit!(matches, "rpc_send_transaction_batch_ms", u64);

    if rpc_send_batch_send_rate_ms > rpc_send_retry_rate_ms {
        eprintln!(
            "The specified rpc-send-batch-ms ({}) is invalid, it must be <= rpc-send-retry-ms ({})",
            rpc_send_batch_send_rate_ms, rpc_send_retry_rate_ms
        );
        exit(1);
    }

    let tps = rpc_send_batch_size as u64 * MILLIS_PER_SECOND / rpc_send_batch_send_rate_ms;
    if tps > send_transaction_service::MAX_TRANSACTION_SENDS_PER_SECOND {
        eprintln!(
            "Either the specified rpc-send-batch-size ({}) or rpc-send-batch-ms ({}) is invalid, \
            'rpc-send-batch-size * 1000 / rpc-send-batch-ms' must be smaller than ({}) .",
            rpc_send_batch_size,
            rpc_send_batch_send_rate_ms,
            send_transaction_service::MAX_TRANSACTION_SENDS_PER_SECOND
        );
        exit(1);
    }
    let full_api = matches.is_present("full_rpc_api");

    let mut validator_config = ValidatorConfig {
        require_tower: matches.is_present("require_tower"),
        tower_storage,
        halt_at_slot: value_t!(matches, "dev_halt_at_slot", Slot).ok(),
        expected_genesis_hash: matches
            .value_of("expected_genesis_hash")
            .map(|s| Hash::from_str(s).unwrap()),
        expected_bank_hash: matches
            .value_of("expected_bank_hash")
            .map(|s| Hash::from_str(s).unwrap()),
        expected_shred_version,
        new_hard_forks: hardforks_of(&matches, "hard_forks"),
        rpc_config: JsonRpcConfig {
            enable_rpc_transaction_history: matches.is_present("enable_rpc_transaction_history"),
            enable_extended_tx_metadata_storage: matches.is_present("enable_cpi_and_log_storage")
                || matches.is_present("enable_extended_tx_metadata_storage"),
            rpc_bigtable_config,
            faucet_addr: matches.value_of("rpc_faucet_addr").map(|address| {
                solana_net_utils::parse_host_port(address).expect("failed to parse faucet address")
            }),
            full_api,
            obsolete_v1_7_api: matches.is_present("obsolete_v1_7_rpc_api"),
            max_multiple_accounts: Some(value_t_or_exit!(
                matches,
                "rpc_max_multiple_accounts",
                usize
            )),
            health_check_slot_distance: value_t_or_exit!(
                matches,
                "health_check_slot_distance",
                u64
            ),
            rpc_threads: value_t_or_exit!(matches, "rpc_threads", usize),
            rpc_niceness_adj: value_t_or_exit!(matches, "rpc_niceness_adj", i8),
            account_indexes: account_indexes.clone(),
            rpc_scan_and_fix_roots: matches.is_present("rpc_scan_and_fix_roots"),
            max_request_body_size: Some(value_t_or_exit!(
                matches,
                "rpc_max_request_body_size",
                usize
            )),
        },
        geyser_plugin_config_files,
        rpc_addrs: value_t!(matches, "rpc_port", u16).ok().map(|rpc_port| {
            (
                SocketAddr::new(rpc_bind_address, rpc_port),
                SocketAddr::new(rpc_bind_address, rpc_port + 1),
                // If additional ports are added, +2 needs to be skipped to avoid a conflict with
                // the websocket port (which is +2) in web3.js This odd port shifting is tracked at
                // https://github.com/solana-labs/solana/issues/12250
            )
        }),
        pubsub_config: PubSubConfig {
            enable_block_subscription: matches.is_present("rpc_pubsub_enable_block_subscription"),
            enable_vote_subscription: matches.is_present("rpc_pubsub_enable_vote_subscription"),
            max_active_subscriptions: value_t_or_exit!(
                matches,
                "rpc_pubsub_max_active_subscriptions",
                usize
            ),
            queue_capacity_items: value_t_or_exit!(
                matches,
                "rpc_pubsub_queue_capacity_items",
                usize
            ),
            queue_capacity_bytes: value_t_or_exit!(
                matches,
                "rpc_pubsub_queue_capacity_bytes",
                usize
            ),
            worker_threads: value_t_or_exit!(matches, "rpc_pubsub_worker_threads", usize),
            notification_threads: if full_api {
                value_of(&matches, "rpc_pubsub_notification_threads")
            } else {
                Some(0)
            },
        },
        voting_disabled: matches.is_present("no_voting") || restricted_repair_only_mode,
        wait_for_supermajority: value_t!(matches, "wait_for_supermajority", Slot).ok(),
        known_validators,
        repair_validators,
        gossip_validators,
        no_rocksdb_compaction,
        rocksdb_compaction_interval,
        rocksdb_max_compaction_jitter,
        wal_recovery_mode,
        poh_verify: !matches.is_present("skip_poh_verify"),
        debug_keys,
        contact_debug_interval,
        send_transaction_service_config: send_transaction_service::Config {
            retry_rate_ms: rpc_send_retry_rate_ms,
            leader_forward_count: value_t_or_exit!(
                matches,
                "rpc_send_transaction_leader_forward_count",
                u64
            ),
            default_max_retries: value_t!(
                matches,
                "rpc_send_transaction_default_max_retries",
                usize
            )
            .ok(),
            service_max_retries: value_t_or_exit!(
                matches,
                "rpc_send_transaction_service_max_retries",
                usize
            ),
            batch_send_rate_ms: rpc_send_batch_send_rate_ms,
            batch_size: rpc_send_batch_size,
        },
        no_poh_speed_test: matches.is_present("no_poh_speed_test"),
        no_os_memory_stats_reporting: matches.is_present("no_os_memory_stats_reporting"),
        no_os_network_stats_reporting: matches.is_present("no_os_network_stats_reporting"),
        no_os_cpu_stats_reporting: matches.is_present("no_os_cpu_stats_reporting"),
        no_os_disk_stats_reporting: matches.is_present("no_os_disk_stats_reporting"),
        poh_pinned_cpu_core: value_of(&matches, "poh_pinned_cpu_core")
            .unwrap_or(poh_service::DEFAULT_PINNED_CPU_CORE),
        poh_hashes_per_batch: value_of(&matches, "poh_hashes_per_batch")
            .unwrap_or(poh_service::DEFAULT_HASHES_PER_BATCH),
        account_indexes,
        accounts_db_caching_enabled: !matches.is_present("no_accounts_db_caching"),
        accounts_db_test_hash_calculation: matches.is_present("accounts_db_test_hash_calculation"),
        accounts_db_config,
        accounts_db_skip_shrink: matches.is_present("accounts_db_skip_shrink"),
        tpu_coalesce_ms,
        no_wait_for_vote_to_start_leader: matches.is_present("no_wait_for_vote_to_start_leader"),
        accounts_shrink_ratio,
        runtime_config: RuntimeConfig {
            bpf_jit: !matches.is_present("no_bpf_jit"),
            log_messages_bytes_limit: value_of(&matches, "log_messages_bytes_limit"),
            ..RuntimeConfig::default()
        },
        staked_nodes_overrides: staked_nodes_overrides.clone(),
        replay_slots_concurrently: matches.is_present("replay_slots_concurrently"),
        ..ValidatorConfig::default()
    };

    let vote_account = pubkey_of(&matches, "vote_account").unwrap_or_else(|| {
        if !validator_config.voting_disabled {
            warn!("--vote-account not specified, validator will not vote");
            validator_config.voting_disabled = true;
        }
        Keypair::new().pubkey()
    });

    let dynamic_port_range =
        solana_net_utils::parse_port_range(matches.value_of("dynamic_port_range").unwrap())
            .expect("invalid dynamic_port_range");

    let account_paths: Vec<PathBuf> =
        if let Ok(account_paths) = values_t!(matches, "account_paths", String) {
            account_paths
                .join(",")
                .split(',')
                .map(PathBuf::from)
                .collect()
        } else {
            vec![ledger_path.join("accounts")]
        };
    let account_shrink_paths: Option<Vec<PathBuf>> =
        values_t!(matches, "account_shrink_path", String)
            .map(|shrink_paths| shrink_paths.into_iter().map(PathBuf::from).collect())
            .ok();

    // Create and canonicalize account paths to avoid issues with symlink creation
    validator_config.account_paths = account_paths
        .into_iter()
        .map(|account_path| {
            match fs::create_dir_all(&account_path).and_then(|_| fs::canonicalize(&account_path)) {
                Ok(account_path) => account_path,
                Err(err) => {
                    eprintln!(
                        "Unable to access account path: {:?}, err: {:?}",
                        account_path, err
                    );
                    exit(1);
                }
            }
        })
        .collect();

    validator_config.account_shrink_paths = account_shrink_paths.map(|paths| {
        paths
            .into_iter()
            .map(|account_path| {
                match fs::create_dir_all(&account_path)
                    .and_then(|_| fs::canonicalize(&account_path))
                {
                    Ok(account_path) => account_path,
                    Err(err) => {
                        eprintln!(
                            "Unable to access account path: {:?}, err: {:?}",
                            account_path, err
                        );
                        exit(1);
                    }
                }
            })
            .collect()
    });

    let maximum_local_snapshot_age = value_t_or_exit!(matches, "maximum_local_snapshot_age", u64);
    let maximum_full_snapshot_archives_to_retain =
        value_t_or_exit!(matches, "maximum_full_snapshots_to_retain", usize);
    let maximum_incremental_snapshot_archives_to_retain =
        value_t_or_exit!(matches, "maximum_incremental_snapshots_to_retain", usize);
    let snapshot_packager_niceness_adj =
        value_t_or_exit!(matches, "snapshot_packager_niceness_adj", i8);
    let minimal_snapshot_download_speed =
        value_t_or_exit!(matches, "minimal_snapshot_download_speed", f32);
    let maximum_snapshot_download_abort =
        value_t_or_exit!(matches, "maximum_snapshot_download_abort", u64);

    let full_snapshot_archives_dir = if matches.is_present("snapshots") {
        PathBuf::from(matches.value_of("snapshots").unwrap())
    } else {
        ledger_path.clone()
    };
    let incremental_snapshot_archives_dir =
        if matches.is_present("incremental_snapshot_archive_path") {
            let incremental_snapshot_archives_dir = PathBuf::from(
                matches
                    .value_of("incremental_snapshot_archive_path")
                    .unwrap(),
            );
            fs::create_dir_all(&incremental_snapshot_archives_dir).unwrap_or_else(|err| {
                eprintln!(
                    "Failed to create incremental snapshot archives directory {:?}: {}",
                    incremental_snapshot_archives_dir.display(),
                    err
                );
                exit(1);
            });
            incremental_snapshot_archives_dir
        } else {
            full_snapshot_archives_dir.clone()
        };
    let bank_snapshots_dir = incremental_snapshot_archives_dir.join("snapshot");
    fs::create_dir_all(&bank_snapshots_dir).unwrap_or_else(|err| {
        eprintln!(
            "Failed to create snapshots directory {:?}: {}",
            bank_snapshots_dir.display(),
            err
        );
        exit(1);
    });

    let archive_format = {
        let archive_format_str = value_t_or_exit!(matches, "snapshot_archive_format", String);
        ArchiveFormat::from_cli_arg(&archive_format_str)
            .unwrap_or_else(|| panic!("Archive format not recognized: {}", archive_format_str))
    };

    let snapshot_version =
        matches
            .value_of("snapshot_version")
            .map_or(SnapshotVersion::default(), |s| {
                s.parse::<SnapshotVersion>().unwrap_or_else(|err| {
                    eprintln!("Error: {}", err);
                    exit(1)
                })
            });

    let incremental_snapshot_interval_slots =
        value_t_or_exit!(matches, "incremental_snapshot_interval_slots", u64);
    let (full_snapshot_archive_interval_slots, incremental_snapshot_archive_interval_slots) =
        if incremental_snapshot_interval_slots > 0 {
            if !matches.is_present("no_incremental_snapshots") {
                (
                    value_t_or_exit!(matches, "full_snapshot_interval_slots", u64),
                    incremental_snapshot_interval_slots,
                )
            } else {
                (incremental_snapshot_interval_slots, Slot::MAX)
            }
        } else {
            (Slot::MAX, Slot::MAX)
        };

    validator_config.snapshot_config = Some(SnapshotConfig {
        usage: if full_snapshot_archive_interval_slots == Slot::MAX {
            SnapshotUsage::LoadOnly
        } else {
            SnapshotUsage::LoadAndGenerate
        },
        full_snapshot_archive_interval_slots,
        incremental_snapshot_archive_interval_slots,
        bank_snapshots_dir,
        full_snapshot_archives_dir: full_snapshot_archives_dir.clone(),
        incremental_snapshot_archives_dir: incremental_snapshot_archives_dir.clone(),
        archive_format,
        snapshot_version,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
        accounts_hash_debug_verify: validator_config.accounts_db_test_hash_calculation,
        packager_thread_niceness_adj: snapshot_packager_niceness_adj,
    });

    validator_config.accounts_hash_interval_slots =
        value_t_or_exit!(matches, "accounts-hash-interval-slots", u64);
    if !is_snapshot_config_valid(
        // SAFETY: Calling `.unwrap()` is safe here because `validator_config.snapshot_config` must
        // be `Some`. The Option<> wrapper will be removed later to solidify this requirement.
        validator_config.snapshot_config.as_ref().unwrap(),
        validator_config.accounts_hash_interval_slots,
    ) {
        eprintln!("Invalid snapshot configuration provided: snapshot intervals are incompatible. \
            \n\t- full snapshot interval MUST be a multiple of accounts hash interval (if enabled) \
            \n\t- incremental snapshot interval MUST be a multiple of accounts hash interval (if enabled) \
            \n\t- full snapshot interval MUST be larger than incremental snapshot interval (if enabled) \
            \nSnapshot configuration values: \
            \n\tfull snapshot interval: {} \
            \n\tincremental snapshot interval: {} \
            \n\taccounts hash interval: {}",
            if full_snapshot_archive_interval_slots == Slot::MAX { "disabled".to_string() } else { full_snapshot_archive_interval_slots.to_string() },
            if incremental_snapshot_archive_interval_slots == Slot::MAX { "disabled".to_string() } else { incremental_snapshot_archive_interval_slots.to_string() },
            validator_config.accounts_hash_interval_slots);

        exit(1);
    }
    if matches.is_present("incremental_snapshots") {
        warn!("--incremental-snapshots is now the default behavior. This flag is deprecated and can be removed from the launch args")
    }

    if matches.is_present("limit_ledger_size") {
        let limit_ledger_size = match matches.value_of("limit_ledger_size") {
            Some(_) => value_t_or_exit!(matches, "limit_ledger_size", u64),
            None => DEFAULT_MAX_LEDGER_SHREDS,
        };
        if limit_ledger_size < DEFAULT_MIN_MAX_LEDGER_SHREDS {
            eprintln!(
                "The provided --limit-ledger-size value was too small, the minimum value is {}",
                DEFAULT_MIN_MAX_LEDGER_SHREDS
            );
            exit(1);
        }
        validator_config.max_ledger_shreds = Some(limit_ledger_size);
    }

    validator_config.ledger_column_options = LedgerColumnOptions {
        compression_type: match matches.value_of("rocksdb_ledger_compression") {
            None => BlockstoreCompressionType::default(),
            Some(ledger_compression_string) => match ledger_compression_string {
                "none" => BlockstoreCompressionType::None,
                "snappy" => BlockstoreCompressionType::Snappy,
                "lz4" => BlockstoreCompressionType::Lz4,
                "zlib" => BlockstoreCompressionType::Zlib,
                _ => panic!(
                    "Unsupported ledger_compression: {}",
                    ledger_compression_string
                ),
            },
        },
        shred_storage_type: match matches.value_of("rocksdb_shred_compaction") {
            None => ShredStorageType::default(),
            Some(shred_compaction_string) => match shred_compaction_string {
                "level" => ShredStorageType::RocksLevel,
                "fifo" => {
                    let shred_storage_size =
                        value_t_or_exit!(matches, "rocksdb_fifo_shred_storage_size", u64);
                    ShredStorageType::rocks_fifo(shred_storage_size)
                }
                _ => panic!(
                    "Unrecognized rocksdb-shred-compaction: {}",
                    shred_compaction_string
                ),
            },
        },
        rocks_perf_sample_interval: value_t_or_exit!(
            matches,
            "rocksdb_perf_sample_interval",
            usize
        ),
    };

    if matches.is_present("halt_on_known_validators_accounts_hash_mismatch") {
        validator_config.halt_on_known_validators_accounts_hash_mismatch = true;
    }

    let public_rpc_addr = matches.value_of("public_rpc_addr").map(|addr| {
        solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse public rpc address: {}", e);
            exit(1);
        })
    });

    if !matches.is_present("no_os_network_limits_test") {
        if SystemMonitorService::check_os_network_limits() {
            info!("OS network limits test passed.");
        } else {
            eprintln!("OS network limit test failed. solana-sys-tuner may be used to configure OS network limits. Bypass check with --no-os-network-limits-test.");
            exit(1);
        }
    }

    let mut ledger_lock = ledger_lockfile(&ledger_path);
    let _ledger_write_guard = lock_ledger(&ledger_path, &mut ledger_lock);

    let start_progress = Arc::new(RwLock::new(ValidatorStartProgress::default()));
    let admin_service_post_init = Arc::new(RwLock::new(None));
    admin_rpc_service::run(
        &ledger_path,
        admin_rpc_service::AdminRpcRequestMetadata {
            rpc_addr: validator_config.rpc_addrs.map(|(rpc_addr, _)| rpc_addr),
            start_time: std::time::SystemTime::now(),
            validator_exit: validator_config.validator_exit.clone(),
            start_progress: start_progress.clone(),
            authorized_voter_keypairs: authorized_voter_keypairs.clone(),
            post_init: admin_service_post_init.clone(),
            tower_storage: validator_config.tower_storage.clone(),
            staked_nodes_overrides,
        },
    );

    let gossip_host: IpAddr = matches
        .value_of("gossip_host")
        .map(|gossip_host| {
            solana_net_utils::parse_host(gossip_host).unwrap_or_else(|err| {
                eprintln!("Failed to parse --gossip-host: {}", err);
                exit(1);
            })
        })
        .unwrap_or_else(|| {
            if !entrypoint_addrs.is_empty() {
                let mut order: Vec<_> = (0..entrypoint_addrs.len()).collect();
                order.shuffle(&mut thread_rng());

                let gossip_host = order.into_iter().find_map(|i| {
                    let entrypoint_addr = &entrypoint_addrs[i];
                    info!(
                        "Contacting {} to determine the validator's public IP address",
                        entrypoint_addr
                    );
                    solana_net_utils::get_public_ip_addr(entrypoint_addr).map_or_else(
                        |err| {
                            eprintln!(
                                "Failed to contact cluster entrypoint {}: {}",
                                entrypoint_addr, err
                            );
                            None
                        },
                        Some,
                    )
                });

                gossip_host.unwrap_or_else(|| {
                    eprintln!("Unable to determine the validator's public IP address");
                    exit(1);
                })
            } else {
                std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
            }
        });

    let gossip_addr = SocketAddr::new(
        gossip_host,
        value_t!(matches, "gossip_port", u16).unwrap_or_else(|_| {
            solana_net_utils::find_available_port_in_range(bind_address, (0, 1)).unwrap_or_else(
                |err| {
                    eprintln!("Unable to find an available gossip port: {}", err);
                    exit(1);
                },
            )
        }),
    );

    let overwrite_tpu_addr = matches.value_of("tpu_host_addr").map(|tpu_addr| {
        solana_net_utils::parse_host_port(tpu_addr).unwrap_or_else(|err| {
            eprintln!("Failed to parse --overwrite-tpu-addr: {}", err);
            exit(1);
        })
    });

    let cluster_entrypoints = entrypoint_addrs
        .iter()
        .map(ContactInfo::new_gossip_entry_point)
        .collect::<Vec<_>>();

    let mut node = Node::new_with_external_ip(
        &identity_keypair.pubkey(),
        &gossip_addr,
        dynamic_port_range,
        bind_address,
        overwrite_tpu_addr,
    );

    if restricted_repair_only_mode {
        let any = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)), 0);
        // When in --restricted_repair_only_mode is enabled only the gossip and repair ports
        // need to be reachable by the entrypoint to respond to gossip pull requests and repair
        // requests initiated by the node.  All other ports are unused.
        node.info.tpu = any;
        node.info.tpu_forwards = any;
        node.info.tvu = any;
        node.info.tvu_forwards = any;
        node.info.serve_repair = any;

        // A node in this configuration shouldn't be an entrypoint to other nodes
        node.sockets.ip_echo = None;
    }

    if !private_rpc {
        if let Some(public_rpc_addr) = public_rpc_addr {
            node.info.rpc = public_rpc_addr;
            node.info.rpc_pubsub = public_rpc_addr;
        } else if let Some((rpc_addr, rpc_pubsub_addr)) = validator_config.rpc_addrs {
            node.info.rpc = SocketAddr::new(node.info.gossip.ip(), rpc_addr.port());
            node.info.rpc_pubsub = SocketAddr::new(node.info.gossip.ip(), rpc_pubsub_addr.port());
        }
    }

    solana_metrics::set_host_id(identity_keypair.pubkey().to_string());
    solana_metrics::set_panic_hook("validator", {
        let version = format!("{:?}", solana_version::version!());
        Some(version)
    });
    solana_entry::entry::init_poh();
    snapshot_utils::remove_tmp_snapshot_archives(&full_snapshot_archives_dir);
    snapshot_utils::remove_tmp_snapshot_archives(&incremental_snapshot_archives_dir);

    let identity_keypair = Arc::new(identity_keypair);

    let should_check_duplicate_instance = !matches.is_present("no_duplicate_instance_check");
    if !cluster_entrypoints.is_empty() {
        bootstrap::rpc_bootstrap(
            &node,
            &identity_keypair,
            &ledger_path,
            &full_snapshot_archives_dir,
            &incremental_snapshot_archives_dir,
            &vote_account,
            authorized_voter_keypairs.clone(),
            &cluster_entrypoints,
            &mut validator_config,
            rpc_bootstrap_config,
            do_port_check,
            use_progress_bar,
            maximum_local_snapshot_age,
            should_check_duplicate_instance,
            &start_progress,
            minimal_snapshot_download_speed,
            maximum_snapshot_download_abort,
            socket_addr_space,
        );
        *start_progress.write().unwrap() = ValidatorStartProgress::Initializing;
    }

    if operation == Operation::Initialize {
        info!("Validator ledger initialization complete");
        return;
    }

    let validator = Validator::new(
        node,
        identity_keypair,
        &ledger_path,
        &vote_account,
        authorized_voter_keypairs,
        cluster_entrypoints,
        &validator_config,
        should_check_duplicate_instance,
        start_progress,
        socket_addr_space,
        tpu_use_quic,
        tpu_connection_pool_size,
        tpu_enable_udp,
    )
    .unwrap_or_else(|e| {
        error!("Failed to start validator: {:?}", e);
        exit(1);
    });
    *admin_service_post_init.write().unwrap() =
        Some(admin_rpc_service::AdminRpcRequestMetadataPostInit {
            bank_forks: validator.bank_forks.clone(),
            cluster_info: validator.cluster_info.clone(),
            vote_account,
        });

    if let Some(filename) = init_complete_file {
        File::create(filename).unwrap_or_else(|_| {
            error!("Unable to create: {}", filename);
            exit(1);
        });
    }
    info!("Validator initialized");
    validator.join();
    info!("Validator exiting..");
}

fn process_account_indexes(matches: &ArgMatches) -> AccountSecondaryIndexes {
    let account_indexes: HashSet<AccountIndex> = matches
        .values_of("account_indexes")
        .unwrap_or_default()
        .map(|value| match value {
            "program-id" => AccountIndex::ProgramId,
            "spl-token-mint" => AccountIndex::SplTokenMint,
            "spl-token-owner" => AccountIndex::SplTokenOwner,
            _ => unreachable!(),
        })
        .collect();

    let account_indexes_include_keys: HashSet<Pubkey> =
        values_t!(matches, "account_index_include_key", Pubkey)
            .unwrap_or_default()
            .iter()
            .cloned()
            .collect();

    let account_indexes_exclude_keys: HashSet<Pubkey> =
        values_t!(matches, "account_index_exclude_key", Pubkey)
            .unwrap_or_default()
            .iter()
            .cloned()
            .collect();

    let exclude_keys = !account_indexes_exclude_keys.is_empty();
    let include_keys = !account_indexes_include_keys.is_empty();

    let keys = if !account_indexes.is_empty() && (exclude_keys || include_keys) {
        let account_indexes_keys = AccountSecondaryIndexesIncludeExclude {
            exclude: exclude_keys,
            keys: if exclude_keys {
                account_indexes_exclude_keys
            } else {
                account_indexes_include_keys
            },
        };
        Some(account_indexes_keys)
    } else {
        None
    };

    AccountSecondaryIndexes {
        keys,
        indexes: account_indexes,
    }
}
