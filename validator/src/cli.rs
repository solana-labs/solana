use {
    clap::{
        crate_description, crate_name, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand,
    },
    log::warn,
    solana_accounts_db::{
        accounts_db::{
            DEFAULT_ACCOUNTS_SHRINK_OPTIMIZE_TOTAL_SPACE, DEFAULT_ACCOUNTS_SHRINK_RATIO,
        },
        hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
    },
    solana_clap_utils::{
        hidden_unless_forced,
        input_validators::{
            is_keypair, is_keypair_or_ask_keyword, is_parsable, is_pow2, is_pubkey,
            is_pubkey_or_keypair, is_slot, is_url_or_moniker, is_valid_percentage, is_within_range,
            validate_maximum_full_snapshot_archives_to_retain,
            validate_maximum_incremental_snapshot_archives_to_retain,
        },
        keypair::SKIP_SEED_PHRASE_VALIDATION_ARG,
    },
    solana_core::{
        banking_trace::{DirByteLimit, BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT},
        validator::{BlockProductionMethod, BlockVerificationMethod},
    },
    solana_faucet::faucet::{self, FAUCET_PORT},
    solana_ledger::use_snapshot_archives_at_startup,
    solana_net_utils::{MINIMUM_VALIDATOR_PORT_RANGE_WIDTH, VALIDATOR_PORT_RANGE},
    solana_rpc::{rpc::MAX_REQUEST_BODY_SIZE, rpc_pubsub_service::PubSubConfig},
    solana_rpc_client_api::request::MAX_MULTIPLE_ACCOUNTS,
    solana_runtime::{
        snapshot_bank_utils::{
            DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
            DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
        },
        snapshot_utils::{
            SnapshotVersion, DEFAULT_ARCHIVE_COMPRESSION,
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN, SUPPORTED_ARCHIVE_COMPRESSION,
        },
    },
    solana_sdk::{
        clock::Slot, epoch_schedule::MINIMUM_SLOTS_PER_EPOCH, hash::Hash, quic::QUIC_PORT_OFFSET,
        rpc_port,
    },
    solana_send_transaction_service::send_transaction_service::{
        self, MAX_BATCH_SEND_RATE_MS, MAX_TRANSACTION_BATCH_SIZE,
    },
    solana_tpu_client::tpu_client::DEFAULT_TPU_CONNECTION_POOL_SIZE,
    std::{path::PathBuf, str::FromStr},
};

const EXCLUDE_KEY: &str = "account-index-exclude-key";
const INCLUDE_KEY: &str = "account-index-include-key";
// The default minimal snapshot download speed (bytes/second)
const DEFAULT_MIN_SNAPSHOT_DOWNLOAD_SPEED: u64 = 10485760;
// The maximum times of snapshot download abort and retry
const MAX_SNAPSHOT_DOWNLOAD_ABORT: u32 = 5;
// We've observed missed leader slots leading to deadlocks on test validator
// with less than 2 ticks per slot.
const MINIMUM_TICKS_PER_SLOT: u64 = 2;

pub fn app<'a>(version: &'a str, default_args: &'a DefaultArgs) -> App<'a, 'a> {
    return App::new(crate_name!()).about(crate_description!())
        .version(version)
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
                .default_value(&default_args.ledger_path)
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
                .validator(port_validator)
                .help("Enable JSON RPC on this port, and the next port for the RPC websocket"),
        )
        .arg(
            Arg::with_name("full_rpc_api")
                .long("full-rpc-api")
                .conflicts_with("minimal_rpc_api")
                .takes_value(false)
                .help("Expose RPC methods for querying chain state and transaction history"),
        )
        .arg(
            Arg::with_name("obsolete_v1_7_rpc_api")
                .long("enable-rpc-obsolete_v1_7")
                .takes_value(false)
                .help("Enable the obsolete RPC methods removed in v1.7"),
        )
        .arg(
            Arg::with_name("private_rpc")
                .long("private-rpc")
                .takes_value(false)
                .help("Do not publish the RPC port for use by others")
        )
        .arg(
            Arg::with_name("no_port_check")
                .long("no-port-check")
                .takes_value(false)
                .hidden(hidden_unless_forced())
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
                .default_value(&default_args.rpc_max_multiple_accounts)
                .help("Override the default maximum accounts accepted by \
                       the getMultipleAccounts JSON RPC method")
        )
        .arg(
            Arg::with_name("health_check_slot_distance")
                .long("health-check-slot-distance")
                .value_name("SLOT_DISTANCE")
                .takes_value(true)
                .default_value(&default_args.health_check_slot_distance)
                .help("Report this validator healthy if its latest optimistically confirmed slot \
                       that has been replayed is no further behind than this number of slots from \
                       the cluster latest optimistically confirmed slot")
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
            Arg::with_name("accounts_hash_cache_path")
                .long("accounts-hash-cache-path")
                .value_name("PATH")
                .takes_value(true)
                .help("Use PATH as accounts hash cache location [default: <LEDGER>/accounts_hash_cache]"),
        )
        .arg(
            Arg::with_name("snapshots")
                .long("snapshots")
                .value_name("DIR")
                .takes_value(true)
                .help("Use DIR as snapshot location [default: --ledger value]"),
        )
        .arg(
            Arg::with_name(use_snapshot_archives_at_startup::cli::NAME)
                .long(use_snapshot_archives_at_startup::cli::LONG_ARG)
                .takes_value(true)
                .possible_values(use_snapshot_archives_at_startup::cli::POSSIBLE_VALUES)
                .default_value(use_snapshot_archives_at_startup::cli::default_value())
                .help(use_snapshot_archives_at_startup::cli::HELP)
                .long_help(use_snapshot_archives_at_startup::cli::LONG_HELP)
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
                .default_value(&default_args.tower_storage)
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
                .default_value(&default_args.etcd_domain_name)
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
            Arg::with_name("public_tpu_addr")
                .long("public-tpu-address")
                .alias("tpu-host-addr")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Specify TPU address to advertise in gossip [default: ask --entrypoint or localhost\
                    when --entrypoint is not provided]"),
        )
        .arg(
            Arg::with_name("public_tpu_forwards_addr")
                .long("public-tpu-forwards-address")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Specify TPU Forwards address to advertise in gossip [default: ask --entrypoint or localhost\
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
                .default_value(&default_args.dynamic_port_range)
                .validator(port_range_validator)
                .help("Range to use for dynamically assigned ports"),
        )
        .arg(
            Arg::with_name("maximum_local_snapshot_age")
                .long("maximum-local-snapshot-age")
                .value_name("NUMBER_OF_SLOTS")
                .takes_value(true)
                .default_value(&default_args.maximum_local_snapshot_age)
                .help("Reuse a local snapshot if it's less than this many \
                       slots behind the highest snapshot available for \
                       download from other validators"),
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
                .default_value(&default_args.incremental_snapshot_archive_interval_slots)
                .help("Number of slots between generating snapshots, \
                      0 to disable snapshots"),
        )
        .arg(
            Arg::with_name("full_snapshot_interval_slots")
                .long("full-snapshot-interval-slots")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(&default_args.full_snapshot_archive_interval_slots)
                .help("Number of slots between generating full snapshots. \
                    Must be a multiple of the incremental snapshot interval.")
        )
        .arg(
            Arg::with_name("maximum_full_snapshots_to_retain")
                .long("maximum-full-snapshots-to-retain")
                .alias("maximum-snapshots-to-retain")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(&default_args.maximum_full_snapshot_archives_to_retain)
                .validator(validate_maximum_full_snapshot_archives_to_retain)
                .help("The maximum number of full snapshot archives to hold on to when purging older snapshots.")
        )
        .arg(
            Arg::with_name("maximum_incremental_snapshots_to_retain")
                .long("maximum-incremental-snapshots-to-retain")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(&default_args.maximum_incremental_snapshot_archives_to_retain)
                .validator(validate_maximum_incremental_snapshot_archives_to_retain)
                .help("The maximum number of incremental snapshot archives to hold on to when purging older snapshots.")
        )
        .arg(
            Arg::with_name("snapshot_packager_niceness_adj")
                .long("snapshot-packager-niceness-adjustment")
                .value_name("ADJUSTMENT")
                .takes_value(true)
                .validator(solana_perf::thread::is_niceness_adjustment_valid)
                .default_value(&default_args.snapshot_packager_niceness_adjustment)
                .help("Add this value to niceness of snapshot packager thread. Negative value \
                      increases priority, positive value decreases priority.")
        )
        .arg(
            Arg::with_name("minimal_snapshot_download_speed")
                .long("minimal-snapshot-download-speed")
                .value_name("MINIMAL_SNAPSHOT_DOWNLOAD_SPEED")
                .takes_value(true)
                .default_value(&default_args.min_snapshot_download_speed)
                .help("The minimal speed of snapshot downloads measured in bytes/second. \
                      If the initial download speed falls below this threshold, the system will \
                      retry the download against a different rpc node."),
        )
        .arg(
            Arg::with_name("maximum_snapshot_download_abort")
                .long("maximum-snapshot-download-abort")
                .value_name("MAXIMUM_SNAPSHOT_DOWNLOAD_ABORT")
                .takes_value(true)
                .default_value(&default_args.max_snapshot_download_abort)
                .help("The maximum number of times to abort and retry when encountering a \
                      slow snapshot download."),
        )
        .arg(
            Arg::with_name("contact_debug_interval")
                .long("contact-debug-interval")
                .value_name("CONTACT_DEBUG_INTERVAL")
                .takes_value(true)
                .default_value(&default_args.contact_debug_interval)
                .help("Milliseconds between printing contact debug from gossip."),
        )
        .arg(
            Arg::with_name("no_poh_speed_test")
                .long("no-poh-speed-test")
                .hidden(hidden_unless_forced())
                .help("Skip the check for PoH speed."),
        )
        .arg(
            Arg::with_name("no_os_network_limits_test")
                .hidden(hidden_unless_forced())
                .long("no-os-network-limits-test")
                .help("Skip checks for OS network limits.")
        )
        .arg(
            Arg::with_name("no_os_memory_stats_reporting")
                .long("no-os-memory-stats-reporting")
                .hidden(hidden_unless_forced())
                .help("Disable reporting of OS memory statistics.")
        )
        .arg(
            Arg::with_name("no_os_network_stats_reporting")
                .long("no-os-network-stats-reporting")
                .hidden(hidden_unless_forced())
                .help("Disable reporting of OS network statistics.")
        )
        .arg(
            Arg::with_name("no_os_cpu_stats_reporting")
                .long("no-os-cpu-stats-reporting")
                .hidden(hidden_unless_forced())
                .help("Disable reporting of OS CPU statistics.")
        )
        .arg(
            Arg::with_name("no_os_disk_stats_reporting")
                .long("no-os-disk-stats-reporting")
                .hidden(hidden_unless_forced())
                .help("Disable reporting of OS disk statistics.")
        )
        .arg(
            Arg::with_name("snapshot_version")
                .long("snapshot-version")
                .value_name("SNAPSHOT_VERSION")
                .validator(is_parsable::<SnapshotVersion>)
                .takes_value(true)
                .default_value(default_args.snapshot_version.into())
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
                .long("rocksdb-shred-compaction")
                .value_name("ROCKSDB_COMPACTION_STYLE")
                .takes_value(true)
                .possible_values(&["level", "fifo"])
                .default_value(&default_args.rocksdb_shred_compaction)
                .help("Controls how RocksDB compacts shreds. \
                       *WARNING*: You will lose your ledger data when you switch between options. \
                       Possible values are: \
                       'level': stores shreds using RocksDB's default (level) compaction. \
                       'fifo': stores shreds under RocksDB's FIFO compaction. \
                           This option is more efficient on disk-write-bytes of the ledger store."),
        )
        .arg(
            Arg::with_name("rocksdb_fifo_shred_storage_size")
                .long("rocksdb-fifo-shred-storage-size")
                .value_name("SHRED_STORAGE_SIZE_BYTES")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .help("The shred storage size in bytes. \
                       The suggested value is at least 50% of your ledger storage size. \
                       If this argument is unspecified, we will assign a proper \
                       value based on --limit-ledger-size.  If --limit-ledger-size \
                       is not presented, it means there is no limitation on the ledger \
                       size and thus rocksdb_fifo_shred_storage_size will also be \
                       unbounded."),
        )
        .arg(
            Arg::with_name("rocksdb_ledger_compression")
                .hidden(hidden_unless_forced())
                .long("rocksdb-ledger-compression")
                .value_name("COMPRESSION_TYPE")
                .takes_value(true)
                .possible_values(&["none", "lz4", "snappy", "zlib"])
                .default_value(&default_args.rocksdb_ledger_compression)
                .help("The compression algorithm that is used to compress \
                       transaction status data.  \
                       Turning on compression can save ~10% of the ledger size."),
        )
        .arg(
            Arg::with_name("rocksdb_perf_sample_interval")
                .hidden(hidden_unless_forced())
                .long("rocksdb-perf-sample-interval")
                .value_name("ROCKS_PERF_SAMPLE_INTERVAL")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value(&default_args.rocksdb_perf_sample_interval)
                .help("Controls how often RocksDB read/write performance sample is collected. \
                       Reads/writes perf samples are collected in 1 / ROCKS_PERF_SAMPLE_INTERVAL sampling rate."),
        )
        .arg(
            Arg::with_name("skip_startup_ledger_verification")
                .long("skip-startup-ledger-verification")
                .takes_value(false)
                .help("Skip ledger verification at validator bootup."),
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
                .hidden(hidden_unless_forced())
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
            Arg::with_name("repair_whitelist")
                .hidden(hidden_unless_forced())
                .long("repair-whitelist")
                .validator(is_pubkey)
                .value_name("VALIDATOR IDENTITY")
                .multiple(true)
                .takes_value(true)
                .help("A list of validators to prioritize repairs from. If specified, repair requests \
                       from validators in the list will be prioritized over requests from other validators. \
                       [default: all validators]")
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
                .hidden(hidden_unless_forced())
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
            Arg::with_name("tpu_connection_pool_size")
                .long("tpu-connection-pool-size")
                .takes_value(true)
                .default_value(&default_args.tpu_connection_pool_size)
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
            Arg::with_name("bind_address")
                .long("bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .default_value(&default_args.bind_address)
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
                .default_value(&default_args.rpc_threads)
                .help("Number of threads to use for servicing RPC requests"),
        )
        .arg(
            Arg::with_name("rpc_niceness_adj")
                .long("rpc-niceness-adjustment")
                .value_name("ADJUSTMENT")
                .takes_value(true)
                .validator(solana_perf::thread::is_niceness_adjustment_valid)
                .default_value(&default_args.rpc_niceness_adjustment)
                .help("Add this value to niceness of RPC threads. Negative value \
                      increases priority, positive value decreases priority.")
        )
        .arg(
            Arg::with_name("rpc_bigtable_timeout")
                .long("rpc-bigtable-timeout")
                .value_name("SECONDS")
                .validator(is_parsable::<u64>)
                .takes_value(true)
                .default_value(&default_args.rpc_bigtable_timeout)
                .help("Number of seconds before timing out RPC requests backed by BigTable"),
        )
        .arg(
            Arg::with_name("rpc_bigtable_instance_name")
                .long("rpc-bigtable-instance-name")
                .takes_value(true)
                .value_name("INSTANCE_NAME")
                .default_value(&default_args.rpc_bigtable_instance_name)
                .help("Name of the Bigtable instance to upload to")
        )
        .arg(
            Arg::with_name("rpc_bigtable_app_profile_id")
                .long("rpc-bigtable-app-profile-id")
                .takes_value(true)
                .value_name("APP_PROFILE_ID")
                .default_value(&default_args.rpc_bigtable_app_profile_id)
                .help("Bigtable application profile id to use in requests")
        )
        .arg(
            Arg::with_name("rpc_bigtable_max_message_size")
                .long("rpc-bigtable-max-message-size")
                .value_name("BYTES")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .default_value(&default_args.rpc_bigtable_max_message_size)
                .help("Max encoding and decoding message size used in Bigtable Grpc client"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_worker_threads")
                .long("rpc-pubsub-worker-threads")
                .takes_value(true)
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .default_value(&default_args.rpc_pubsub_worker_threads)
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
                .hidden(hidden_unless_forced())
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
                .hidden(hidden_unless_forced())
                .help("The maximum length in bytes of acceptable incoming frames. Messages longer \
                       than this will be rejected. (Obsolete)"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_in_buffer_capacity")
                .long("rpc-pubsub-max-in-buffer-capacity")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .hidden(hidden_unless_forced())
                .help("The maximum size in bytes to which the incoming websocket buffer can grow. \
                      (Obsolete)"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_out_buffer_capacity")
                .long("rpc-pubsub-max-out-buffer-capacity")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .hidden(hidden_unless_forced())
                .help("The maximum size in bytes to which the outgoing websocket buffer can grow. \
                       (Obsolete)"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_active_subscriptions")
                .long("rpc-pubsub-max-active-subscriptions")
                .takes_value(true)
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .default_value(&default_args.rpc_pubsub_max_active_subscriptions)
                .help("The maximum number of active subscriptions that RPC PubSub will accept \
                       across all connections."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_queue_capacity_items")
                .long("rpc-pubsub-queue-capacity-items")
                .takes_value(true)
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .default_value(&default_args.rpc_pubsub_queue_capacity_items)
                .help("The maximum number of notifications that RPC PubSub will store \
                       across all connections."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_queue_capacity_bytes")
                .long("rpc-pubsub-queue-capacity-bytes")
                .takes_value(true)
                .value_name("BYTES")
                .validator(is_parsable::<usize>)
                .default_value(&default_args.rpc_pubsub_queue_capacity_bytes)
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
                .default_value(&default_args.rpc_send_transaction_retry_ms)
                .help("The rate at which transactions sent via rpc service are retried."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_batch_ms")
                .long("rpc-send-batch-ms")
                .value_name("MILLISECS")
                .hidden(hidden_unless_forced())
                .takes_value(true)
                .validator(|s| is_within_range(s, 1..=MAX_BATCH_SEND_RATE_MS))
                .default_value(&default_args.rpc_send_transaction_batch_ms)
                .help("The rate at which transactions sent via rpc service are sent in batch."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_leader_forward_count")
                .long("rpc-send-leader-count")
                .value_name("NUMBER")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .default_value(&default_args.rpc_send_transaction_leader_forward_count)
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
                .default_value(&default_args.rpc_send_transaction_service_max_retries)
                .help("The maximum number of transaction broadcast retries, regardless of requested value."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_batch_size")
                .long("rpc-send-batch-size")
                .value_name("NUMBER")
                .hidden(hidden_unless_forced())
                .takes_value(true)
                .validator(|s| is_within_range(s, 1..=MAX_TRANSACTION_BATCH_SIZE))
                .default_value(&default_args.rpc_send_transaction_batch_size)
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
                .default_value(&default_args.rpc_max_request_body_size)
                .help("The maximum request body size accepted by rpc service"),
        )
        .arg(
            Arg::with_name("enable_accountsdb_repl")
                .long("enable-accountsdb-repl")
                .takes_value(false)
                .hidden(hidden_unless_forced())
                .help("Enable AccountsDb Replication"),
        )
        .arg(
            Arg::with_name("accountsdb_repl_bind_address")
                .long("accountsdb-repl-bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .hidden(hidden_unless_forced())
                .help("IP address to bind the AccountsDb Replication port [default: use --bind-address]"),
        )
        .arg(
            Arg::with_name("accountsdb_repl_port")
                .long("accountsdb-repl-port")
                .value_name("PORT")
                .takes_value(true)
                .validator(port_validator)
                .hidden(hidden_unless_forced())
                .help("Enable AccountsDb Replication Service on this port"),
        )
        .arg(
            Arg::with_name("accountsdb_repl_threads")
                .long("accountsdb-repl-threads")
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .default_value(&default_args.accountsdb_repl_threads)
                .hidden(hidden_unless_forced())
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
            Arg::with_name("snapshot_archive_format")
                .long("snapshot-archive-format")
                .alias("snapshot-compression") // Legacy name used by Solana v1.5.x and older
                .possible_values(SUPPORTED_ARCHIVE_COMPRESSION)
                .default_value(&default_args.snapshot_archive_format)
                .value_name("ARCHIVE_TYPE")
                .takes_value(true)
                .help("Snapshot archive format to use."),
        )
        .arg(
            Arg::with_name("max_genesis_archive_unpacked_size")
                .long("max-genesis-archive-unpacked-size")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(&default_args.genesis_archive_unpacked_size)
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
            Arg::with_name("poh_pinned_cpu_core")
                .hidden(hidden_unless_forced())
                .long("experimental-poh-pinned-cpu-core")
                .takes_value(true)
                .value_name("CPU_CORE_INDEX")
                .validator(|s| {
                    let core_index = usize::from_str(&s).map_err(|e| e.to_string())?;
                    let max_index = core_affinity::get_core_ids().map(|cids| cids.len() - 1).unwrap_or(0);
                    if core_index > max_index {
                        return Err(format!("core index must be in the range [0, {max_index}]"));
                    }
                    Ok(())
                })
                .help("EXPERIMENTAL: Specify which CPU core PoH is pinned to"),
        )
        .arg(
            Arg::with_name("poh_hashes_per_batch")
                .hidden(hidden_unless_forced())
                .long("poh-hashes-per-batch")
                .takes_value(true)
                .value_name("NUM")
                .help("Specify hashes per batch in PoH service"),
        )
        .arg(
            Arg::with_name("process_ledger_before_services")
                .long("process-ledger-before-services")
                .hidden(hidden_unless_forced())
                .help("Process the local ledger fully before starting networking services")
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
            Arg::with_name("accounts_db_verify_refcounts")
                .long("accounts-db-verify-refcounts")
                .help("Debug option to scan all append vecs and verify account index refcounts prior to clean")
                .hidden(hidden_unless_forced())
        )
        .arg(
            Arg::with_name("no_skip_initial_accounts_db_clean")
                .long("no-skip-initial-accounts-db-clean")
                .help("Do not skip the initial cleaning of accounts when verifying snapshot bank")
                .hidden(hidden_unless_forced())
                .conflicts_with("accounts_db_skip_shrink")
        )
        .arg(
            Arg::with_name("accounts_db_create_ancient_storage_packed")
                .long("accounts-db-create-ancient-storage-packed")
                .help("Create ancient storages in one shot instead of appending.")
                .hidden(hidden_unless_forced()),
            )
        .arg(
            Arg::with_name("accounts_db_ancient_append_vecs")
                .long("accounts-db-ancient-append-vecs")
                .value_name("SLOT-OFFSET")
                .validator(is_parsable::<i64>)
                .takes_value(true)
                .help("AppendVecs that are older than (slots_per_epoch - SLOT-OFFSET) are squashed together.")
                .hidden(hidden_unless_forced()),
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
            Arg::with_name("accounts_index_bins")
                .long("accounts-index-bins")
                .value_name("BINS")
                .validator(is_pow2)
                .takes_value(true)
                .help("Number of bins to divide the accounts index into"),
        )
        .arg(
            Arg::with_name("partitioned_epoch_rewards_compare_calculation")
                .long("partitioned-epoch-rewards-compare-calculation")
                .takes_value(false)
                .help("Do normal epoch rewards distribution, but also calculate rewards using the partitioned rewards code path and compare the resulting vote and stake accounts")
                .hidden(hidden_unless_forced())
        )
        .arg(
            Arg::with_name("partitioned_epoch_rewards_force_enable_single_slot")
                .long("partitioned-epoch-rewards-force-enable-single-slot")
                .takes_value(false)
                .help("Force the partitioned rewards distribution, but distribute all rewards in the first slot in the epoch. This should match consensus with the normal rewards distribution.")
                .conflicts_with("partitioned_epoch_rewards_compare_calculation")
                .hidden(hidden_unless_forced())
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
            .default_value(&default_args.accounts_filler_count)
            .help("How many accounts to add to stress the system. Accounts are ignored in operations related to correctness.")
            .hidden(hidden_unless_forced()))
        .arg(Arg::with_name("accounts_filler_size")
            .long("accounts-filler-size")
            .value_name("BYTES")
            .validator(is_parsable::<usize>)
            .takes_value(true)
            .default_value(&default_args.accounts_filler_size)
            .requires("accounts_filler_count")
            .help("Size per filler account in bytes.")
            .hidden(hidden_unless_forced()))
        .arg(
            Arg::with_name("accounts_db_test_hash_calculation")
                .long("accounts-db-test-hash-calculation")
                .help("Enables testing of hash calculation using stores in \
                      AccountsHashVerifier. This has a computational cost."),
        )
        .arg(
            Arg::with_name("accounts_shrink_optimize_total_space")
                .long("accounts-shrink-optimize-total-space")
                .takes_value(true)
                .value_name("BOOLEAN")
                .default_value(&default_args.accounts_shrink_optimize_total_space)
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
                .default_value(&default_args.accounts_shrink_ratio)
                .help("Specifies the shrink ratio for the accounts to be shrunk. \
                       The shrink ratio is defined as the ratio of the bytes alive over the  \
                       total bytes used. If the account's shrink ratio is less than this ratio \
                       it becomes a candidate for shrinking. The value must between 0. and 1.0 \
                       inclusive."),
        )
        .arg(
            Arg::with_name("allow_private_addr")
                .long("allow-private-addr")
                .takes_value(false)
                .help("Allow contacting private ip addresses")
                .hidden(hidden_unless_forced()),
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
        .arg(
            Arg::with_name("banking_trace_dir_byte_limit")
                // expose friendly alternative name to cli than internal
                // implementation-oriented one
                .long("enable-banking-trace")
                .value_name("BYTES")
                .validator(is_parsable::<DirByteLimit>)
                .takes_value(true)
                // Firstly, zero limit value causes tracer to be disabled
                // altogether, intuitively. On the other hand, this non-zero
                // default doesn't enable banking tracer unless this flag is
                // explicitly given, similar to --limit-ledger-size.
                // see configure_banking_trace_dir_byte_limit() for this.
                .default_value(&default_args.banking_trace_dir_byte_limit)
                .help("Enables the banking trace explicitly, which is enabled by default and \
                       writes trace files for simulate-leader-blocks, retaining up to the default \
                       or specified total bytes in the ledger. This flag can be used to override \
                       its byte limit.")
        )
        .arg(
            Arg::with_name("disable_banking_trace")
                .long("disable-banking-trace")
                .conflicts_with("banking_trace_dir_byte_limit")
                .takes_value(false)
                .help("Disables the banking trace")
        )
        .arg(
            Arg::with_name("block_verification_method")
                .long("block-verification-method")
                .hidden(hidden_unless_forced())
                .value_name("METHOD")
                .takes_value(true)
                .possible_values(BlockVerificationMethod::cli_names())
                .help(BlockVerificationMethod::cli_message())
        )
        .arg(
            Arg::with_name("block_production_method")
                .long("block-production-method")
                .hidden(hidden_unless_forced())
                .value_name("METHOD")
                .takes_value(true)
                .possible_values(BlockProductionMethod::cli_names())
                .help(BlockProductionMethod::cli_message())
        )
        .args(&get_deprecated_arguments())
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
                        .default_value(&default_args.exit_min_idle_time)
                        .help("Minimum time that the validator should not be leader before restarting")
                )
                .arg(
                    Arg::with_name("max_delinquent_stake")
                        .long("max-delinquent-stake")
                        .takes_value(true)
                        .validator(is_valid_percentage)
                        .default_value(&default_args.exit_max_delinquent_stake)
                        .value_name("PERCENT")
                        .help("The maximum delinquent stake % permitted for an exit")
                )
                .arg(
                    Arg::with_name("skip_new_snapshot_check")
                        .long("skip-new-snapshot-check")
                        .help("Skip check for a new snapshot")
                )
                .arg(
                    Arg::with_name("skip_health_check")
                        .long("skip-health-check")
                        .help("Skip health check")
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
            SubCommand::with_name("repair-whitelist")
                .about("Manage the validator's repair protocol whitelist")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .setting(AppSettings::InferSubcommands)
                .subcommand(
                    SubCommand::with_name("get")
                        .about("Display the validator's repair protocol whitelist")
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
                    SubCommand::with_name("set")
                        .about("Set the validator's repair protocol whitelist")
                        .setting(AppSettings::ArgRequiredElseHelp)
                        .arg(
                            Arg::with_name("whitelist")
                            .long("whitelist")
                            .validator(is_pubkey)
                            .value_name("VALIDATOR IDENTITY")
                            .multiple(true)
                            .takes_value(true)
                            .help("Set the validator's repair protocol whitelist")
                        )
                        .after_help("Note: repair protocol whitelist changes only apply to the currently \
                                    running validator instance")
                )
                .subcommand(
                    SubCommand::with_name("remove-all")
                        .about("Clear the validator's repair protocol whitelist")
                        .after_help("Note: repair protocol whitelist changes only apply to the currently \
                                    running validator instance")
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
            SubCommand::with_name("plugin")
                .about("Manage and view geyser plugins")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .setting(AppSettings::InferSubcommands)
                .subcommand(
                    SubCommand::with_name("list")
                        .about("List all current running gesyer plugins")
                )
                .subcommand(
                    SubCommand::with_name("unload")
                        .about("Unload a particular gesyer plugin. You must specify the gesyer plugin name")
                        .arg(
                            Arg::with_name("name")
                                .required(true)
                                .takes_value(true)
                        )
                )
                .subcommand(
                    SubCommand::with_name("reload")
                        .about("Reload a particular gesyer plugin. You must specify the gesyer plugin name and the new config path")
                        .arg(
                            Arg::with_name("name")
                                .required(true)
                                .takes_value(true)
                        )
                        .arg(
                            Arg::with_name("config")
                                .required(true)
                                .takes_value(true)
                        )
                )
                .subcommand(
                    SubCommand::with_name("load")
                        .about("Load a new gesyer plugin. You must specify the config path. Fails if overwriting (use reload)")
                        .arg(
                            Arg::with_name("config")
                                .required(true)
                                .takes_value(true)
                        )
                )
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
                        .default_value(&default_args.wait_for_restart_window_min_idle_time)
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
                        .default_value(&default_args.wait_for_restart_window_max_delinquent_stake)
                        .value_name("PERCENT")
                        .help("The maximum delinquent stake % permitted for a restart")
                )
                .arg(
                    Arg::with_name("skip_new_snapshot_check")
                        .long("skip-new-snapshot-check")
                        .help("Skip check for a new snapshot")
                )
                .arg(
                    Arg::with_name("skip_health_check")
                        .long("skip-health-check")
                        .help("Skip health check")
                )
                .after_help("Note: If this command exits with a non-zero status \
                         then this not a good time for a restart")
        ).
        subcommand(
            SubCommand::with_name("set-public-address")
                .about("Specify addresses to advertise in gossip")
                .arg(
                    Arg::with_name("tpu_addr")
                        .long("tpu")
                        .value_name("HOST:PORT")
                        .takes_value(true)
                        .validator(solana_net_utils::is_host_port)
                        .help("TPU address to advertise in gossip")
                )
                .arg(
                    Arg::with_name("tpu_forwards_addr")
                        .long("tpu-forwards")
                        .value_name("HOST:PORT")
                        .takes_value(true)
                        .validator(solana_net_utils::is_host_port)
                        .help("TPU Forwards address to advertise in gossip")
                )
                .group(
                    ArgGroup::with_name("set_public_address_details")
                        .args(&["tpu_addr", "tpu_forwards_addr"])
                        .required(true)
                        .multiple(true)
                )
                .after_help("Note: At least one arg must be used. Using multiple is ok"),
        );
}

/// Deprecated argument description should be moved into the [`deprecated_arguments()`] function,
/// expressed as an instance of this type.
struct DeprecatedArg {
    /// Deprecated argument description, moved here as is.
    ///
    /// `hidden` property will be modified by [`deprecated_arguments()`] to only show this argument
    /// if [`hidden_unless_forced()`] says they should be displayed.
    arg: Arg<'static, 'static>,

    /// If simply replaced by a different argument, this is the name of the replacement.
    ///
    /// Content should be an argument name, as presented to users.
    replaced_by: Option<&'static str>,

    /// An explanation to be shown to the user if they still use this argument.
    ///
    /// Content should be a complete sentence or several, ending with a period.
    usage_warning: Option<&'static str>,
}

fn deprecated_arguments() -> Vec<DeprecatedArg> {
    let mut res = vec![];

    // This macro reduces indentation and removes some noise from the argument declaration list.
    macro_rules! add_arg {
        (
            $arg:expr
            $( , replaced_by: $replaced_by:expr )?
            $( , usage_warning: $usage_warning:expr )?
            $(,)?
        ) => {
            let replaced_by = add_arg!(@into-option $( $replaced_by )?);
            let usage_warning = add_arg!(@into-option $( $usage_warning )?);
            res.push(DeprecatedArg {
                arg: $arg,
                replaced_by,
                usage_warning,
            });
        };

        (@into-option) => { None };
        (@into-option $v:expr) => { Some($v) };
    }

    add_arg!(Arg::with_name("accounts_db_caching_enabled").long("accounts-db-caching-enabled"));
    add_arg!(
        Arg::with_name("accounts_db_index_hashing")
            .long("accounts-db-index-hashing")
            .help(
                "Enables the use of the index in hash calculation in \
                 AccountsHashVerifier/Accounts Background Service.",
            ),
        usage_warning: "The accounts hash is only calculated without using the index.",
    );
    add_arg!(
        Arg::with_name("accounts_db_skip_shrink")
            .long("accounts-db-skip-shrink")
            .help("Enables faster starting of validators by skipping startup clean and shrink."),
        usage_warning: "Enabled by default",
    );
    add_arg!(Arg::with_name("accounts_hash_interval_slots")
        .long("accounts-hash-interval-slots")
        .value_name("NUMBER")
        .takes_value(true)
        .help("Number of slots between verifying accounts hash.")
        .validator(|val| {
            if val.eq("0") {
                Err(String::from("Accounts hash interval cannot be zero"))
            } else {
                Ok(())
            }
        }));
    add_arg!(Arg::with_name("disable_accounts_disk_index")
        .long("disable-accounts-disk-index")
        .help("Disable the disk-based accounts index if it is enabled by default.")
        .conflicts_with("accounts_index_memory_limit_mb"));
    add_arg!(
        Arg::with_name("disable_quic_servers")
            .long("disable-quic-servers")
            .takes_value(false),
        usage_warning: "The quic server cannot be disabled.",
    );
    add_arg!(
        Arg::with_name("enable_cpi_and_log_storage")
            .long("enable-cpi-and-log-storage")
            .requires("enable_rpc_transaction_history")
            .takes_value(false)
            .help(
                "Include CPI inner instructions, logs and return data in the historical \
                 transaction info stored",
            ),
        replaced_by: "enable-extended-tx-metadata-storage",
    );
    add_arg!(
        Arg::with_name("enable_quic_servers")
            .long("enable-quic-servers"),
        usage_warning: "The quic server is now enabled by default.",
    );
    add_arg!(
        Arg::with_name("halt_on_known_validators_accounts_hash_mismatch")
            .alias("halt-on-trusted-validators-accounts-hash-mismatch")
            .long("halt-on-known-validators-accounts-hash-mismatch")
            .requires("known_validators")
            .takes_value(false)
            .help("Abort the validator if a bank hash mismatch is detected within known validator set"),
    );
    add_arg!(Arg::with_name("incremental_snapshots")
        .long("incremental-snapshots")
        .takes_value(false)
        .conflicts_with("no_incremental_snapshots")
        .help("Enable incremental snapshots")
        .long_help(
            "Enable incremental snapshots by setting this flag.  When enabled, \
                 --snapshot-interval-slots will set the incremental snapshot interval. To set the
                 full snapshot interval, use --full-snapshot-interval-slots.",
        ));
    add_arg!(Arg::with_name("minimal_rpc_api")
        .long("minimal-rpc-api")
        .takes_value(false)
        .help("Only expose the RPC methods required to serve snapshots to other nodes"));
    add_arg!(
        Arg::with_name("no_accounts_db_index_hashing")
            .long("no-accounts-db-index-hashing")
            .help(
                "This is obsolete. See --accounts-db-index-hashing. \
                   Disables the use of the index in hash calculation in \
                   AccountsHashVerifier/Accounts Background Service.",
            ),
        usage_warning: "The accounts hash is only calculated without using the index.",
    );
    add_arg!(
        Arg::with_name("no_check_vote_account")
            .long("no-check-vote-account")
            .takes_value(false)
            .conflicts_with("no_voting")
            .requires("entrypoint")
            .help("Skip the RPC vote account sanity check"),
        usage_warning: "Vote account sanity checks are no longer performed by default.",
    );
    add_arg!(Arg::with_name("no_rocksdb_compaction")
        .long("no-rocksdb-compaction")
        .takes_value(false)
        .help("Disable manual compaction of the ledger database"));
    add_arg!(Arg::with_name("rocksdb_compaction_interval")
        .long("rocksdb-compaction-interval-slots")
        .value_name("ROCKSDB_COMPACTION_INTERVAL_SLOTS")
        .takes_value(true)
        .help("Number of slots between compacting ledger"));
    add_arg!(Arg::with_name("rocksdb_max_compaction_jitter")
        .long("rocksdb-max-compaction-jitter-slots")
        .value_name("ROCKSDB_MAX_COMPACTION_JITTER_SLOTS")
        .takes_value(true)
        .help("Introduce jitter into the compaction to offset compaction operation"));
    add_arg!(
        Arg::with_name("skip_poh_verify")
            .long("skip-poh-verify")
            .takes_value(false)
            .help("Skip ledger verification at validator bootup."),
        replaced_by: "skip-startup-ledger-verification",
    );

    res
}

// Helper to add arguments that are no longer used but are being kept around to avoid breaking
// validator startup commands.
fn get_deprecated_arguments() -> Vec<Arg<'static, 'static>> {
    deprecated_arguments()
        .into_iter()
        .map(|info| {
            let arg = info.arg;
            // Hide all deprecated arguments by default.
            arg.hidden(hidden_unless_forced())
        })
        .collect()
}

pub fn warn_for_deprecated_arguments(matches: &ArgMatches) {
    for DeprecatedArg {
        arg,
        replaced_by,
        usage_warning,
    } in deprecated_arguments().into_iter()
    {
        if matches.is_present(arg.b.name) {
            let mut msg = format!("--{} is deprecated", arg.b.name.replace('_', "-"));
            if let Some(replaced_by) = replaced_by {
                msg.push_str(&format!(", please use --{replaced_by}"));
            }
            msg.push('.');
            if let Some(usage_warning) = usage_warning {
                msg.push_str(&format!("  {usage_warning}"));
                if !msg.ends_with('.') {
                    msg.push('.');
                }
            }
            warn!("{}", msg);
        }
    }
}

pub struct DefaultArgs {
    pub bind_address: String,
    pub dynamic_port_range: String,
    pub ledger_path: String,

    pub genesis_archive_unpacked_size: String,
    pub health_check_slot_distance: String,
    pub tower_storage: String,
    pub etcd_domain_name: String,
    pub send_transaction_service_config: send_transaction_service::Config,

    pub rpc_max_multiple_accounts: String,
    pub rpc_pubsub_max_active_subscriptions: String,
    pub rpc_pubsub_queue_capacity_items: String,
    pub rpc_pubsub_queue_capacity_bytes: String,
    pub rpc_send_transaction_retry_ms: String,
    pub rpc_send_transaction_batch_ms: String,
    pub rpc_send_transaction_leader_forward_count: String,
    pub rpc_send_transaction_service_max_retries: String,
    pub rpc_send_transaction_batch_size: String,
    pub rpc_threads: String,
    pub rpc_niceness_adjustment: String,
    pub rpc_bigtable_timeout: String,
    pub rpc_bigtable_instance_name: String,
    pub rpc_bigtable_app_profile_id: String,
    pub rpc_bigtable_max_message_size: String,
    pub rpc_max_request_body_size: String,
    pub rpc_pubsub_worker_threads: String,

    pub maximum_local_snapshot_age: String,
    pub maximum_full_snapshot_archives_to_retain: String,
    pub maximum_incremental_snapshot_archives_to_retain: String,
    pub snapshot_packager_niceness_adjustment: String,
    pub full_snapshot_archive_interval_slots: String,
    pub incremental_snapshot_archive_interval_slots: String,
    pub min_snapshot_download_speed: String,
    pub max_snapshot_download_abort: String,

    pub contact_debug_interval: String,

    pub accounts_filler_count: String,
    pub accounts_filler_size: String,
    pub accountsdb_repl_threads: String,

    pub snapshot_version: SnapshotVersion,
    pub snapshot_archive_format: String,

    pub rocksdb_shred_compaction: String,
    pub rocksdb_ledger_compression: String,
    pub rocksdb_perf_sample_interval: String,

    pub accounts_shrink_optimize_total_space: String,
    pub accounts_shrink_ratio: String,
    pub tpu_connection_pool_size: String,

    // Exit subcommand
    pub exit_min_idle_time: String,
    pub exit_max_delinquent_stake: String,

    // Wait subcommand
    pub wait_for_restart_window_min_idle_time: String,
    pub wait_for_restart_window_max_delinquent_stake: String,

    pub banking_trace_dir_byte_limit: String,
}

impl DefaultArgs {
    pub fn new() -> Self {
        let default_send_transaction_service_config = send_transaction_service::Config::default();

        DefaultArgs {
            bind_address: "0.0.0.0".to_string(),
            ledger_path: "ledger".to_string(),
            dynamic_port_range: format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1),
            maximum_local_snapshot_age: "2500".to_string(),
            genesis_archive_unpacked_size: MAX_GENESIS_ARCHIVE_UNPACKED_SIZE.to_string(),
            rpc_max_multiple_accounts: MAX_MULTIPLE_ACCOUNTS.to_string(),
            health_check_slot_distance: "150".to_string(),
            tower_storage: "file".to_string(),
            etcd_domain_name: "localhost".to_string(),
            rpc_pubsub_max_active_subscriptions: PubSubConfig::default()
                .max_active_subscriptions
                .to_string(),
            rpc_pubsub_queue_capacity_items: PubSubConfig::default()
                .queue_capacity_items
                .to_string(),
            rpc_pubsub_queue_capacity_bytes: PubSubConfig::default()
                .queue_capacity_bytes
                .to_string(),
            send_transaction_service_config: send_transaction_service::Config::default(),
            rpc_send_transaction_retry_ms: default_send_transaction_service_config
                .retry_rate_ms
                .to_string(),
            rpc_send_transaction_batch_ms: default_send_transaction_service_config
                .batch_send_rate_ms
                .to_string(),
            rpc_send_transaction_leader_forward_count: default_send_transaction_service_config
                .leader_forward_count
                .to_string(),
            rpc_send_transaction_service_max_retries: default_send_transaction_service_config
                .service_max_retries
                .to_string(),
            rpc_send_transaction_batch_size: default_send_transaction_service_config
                .batch_size
                .to_string(),
            rpc_threads: num_cpus::get().to_string(),
            rpc_niceness_adjustment: "0".to_string(),
            rpc_bigtable_timeout: "30".to_string(),
            rpc_bigtable_instance_name: solana_storage_bigtable::DEFAULT_INSTANCE_NAME.to_string(),
            rpc_bigtable_app_profile_id: solana_storage_bigtable::DEFAULT_APP_PROFILE_ID
                .to_string(),
            rpc_bigtable_max_message_size: solana_storage_bigtable::DEFAULT_MAX_MESSAGE_SIZE
                .to_string(),
            rpc_pubsub_worker_threads: "4".to_string(),
            accountsdb_repl_threads: num_cpus::get().to_string(),
            accounts_filler_count: "0".to_string(),
            accounts_filler_size: "0".to_string(),
            maximum_full_snapshot_archives_to_retain: DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN
                .to_string(),
            maximum_incremental_snapshot_archives_to_retain:
                DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN.to_string(),
            snapshot_packager_niceness_adjustment: "0".to_string(),
            full_snapshot_archive_interval_slots: DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS
                .to_string(),
            incremental_snapshot_archive_interval_slots:
                DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS.to_string(),
            min_snapshot_download_speed: DEFAULT_MIN_SNAPSHOT_DOWNLOAD_SPEED.to_string(),
            max_snapshot_download_abort: MAX_SNAPSHOT_DOWNLOAD_ABORT.to_string(),
            snapshot_archive_format: DEFAULT_ARCHIVE_COMPRESSION.to_string(),
            contact_debug_interval: "120000".to_string(),
            snapshot_version: SnapshotVersion::default(),
            rocksdb_shred_compaction: "level".to_string(),
            rocksdb_ledger_compression: "none".to_string(),
            rocksdb_perf_sample_interval: "0".to_string(),
            accounts_shrink_optimize_total_space: DEFAULT_ACCOUNTS_SHRINK_OPTIMIZE_TOTAL_SPACE
                .to_string(),
            accounts_shrink_ratio: DEFAULT_ACCOUNTS_SHRINK_RATIO.to_string(),
            tpu_connection_pool_size: DEFAULT_TPU_CONNECTION_POOL_SIZE.to_string(),
            rpc_max_request_body_size: MAX_REQUEST_BODY_SIZE.to_string(),
            exit_min_idle_time: "10".to_string(),
            exit_max_delinquent_stake: "5".to_string(),
            wait_for_restart_window_min_idle_time: "10".to_string(),
            wait_for_restart_window_max_delinquent_stake: "5".to_string(),
            banking_trace_dir_byte_limit: BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT.to_string(),
        }
    }
}

impl Default for DefaultArgs {
    fn default() -> Self {
        Self::new()
    }
}

pub fn port_validator(port: String) -> Result<(), String> {
    port.parse::<u16>()
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

pub fn port_range_validator(port_range: String) -> Result<(), String> {
    if let Some((start, end)) = solana_net_utils::parse_port_range(&port_range) {
        if end - start < MINIMUM_VALIDATOR_PORT_RANGE_WIDTH {
            Err(format!(
                "Port range is too small.  Try --dynamic-port-range {}-{}",
                start,
                start + MINIMUM_VALIDATOR_PORT_RANGE_WIDTH
            ))
        } else if end.checked_add(QUIC_PORT_OFFSET).is_none() {
            Err("Invalid dynamic_port_range.".to_string())
        } else {
            Ok(())
        }
    } else {
        Err("Invalid port range".to_string())
    }
}

fn hash_validator(hash: String) -> Result<(), String> {
    Hash::from_str(&hash)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// Test validator

pub fn test_app<'a>(version: &'a str, default_args: &'a DefaultTestArgs) -> App<'a, 'a> {
    return App::new("solana-test-validator")
        .about("Test Validator")
        .version(version)
        .arg({
            let arg = Arg::with_name("config_file")
                .short("C")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *solana_cli_config::CONFIG_FILE {
                arg.default_value(config_file)
            } else {
                arg
            }
        })
        .arg(
            Arg::with_name("json_rpc_url")
                .short("u")
                .long("url")
                .value_name("URL_OR_MONIKER")
                .takes_value(true)
                .validator(is_url_or_moniker)
                .help(
                    "URL for Solana's JSON RPC or moniker (or their first letter): \
                   [mainnet-beta, testnet, devnet, localhost]",
                ),
        )
        .arg(
            Arg::with_name("mint_address")
                .long("mint")
                .value_name("PUBKEY")
                .validator(is_pubkey)
                .takes_value(true)
                .help(
                    "Address of the mint account that will receive tokens \
                       created at genesis.  If the ledger already exists then \
                       this parameter is silently ignored [default: client keypair]",
                ),
        )
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .default_value("test-ledger")
                .help("Use DIR as ledger location"),
        )
        .arg(
            Arg::with_name("reset")
                .short("r")
                .long("reset")
                .takes_value(false)
                .help(
                    "Reset the ledger to genesis if it exists. \
                       By default the validator will resume an existing ledger (if present)",
                ),
        )
        .arg(
            Arg::with_name("quiet")
                .short("q")
                .long("quiet")
                .takes_value(false)
                .conflicts_with("log")
                .help("Quiet mode: suppress normal output"),
        )
        .arg(
            Arg::with_name("log")
                .long("log")
                .takes_value(false)
                .conflicts_with("quiet")
                .help("Log mode: stream the validator log"),
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
            Arg::with_name("faucet_port")
                .long("faucet-port")
                .value_name("PORT")
                .takes_value(true)
                .default_value(&default_args.faucet_port)
                .validator(port_validator)
                .help("Enable the faucet on this port"),
        )
        .arg(
            Arg::with_name("rpc_port")
                .long("rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .default_value(&default_args.rpc_port)
                .validator(port_validator)
                .help("Enable JSON RPC on this port, and the next port for the RPC websocket"),
        )
        .arg(
            Arg::with_name("enable_rpc_bigtable_ledger_storage")
                .long("enable-rpc-bigtable-ledger-storage")
                .takes_value(false)
                .hidden(hidden_unless_forced())
                .help("Fetch historical transaction info from a BigTable instance \
                       as a fallback to local ledger data"),
        )
        .arg(
            Arg::with_name("rpc_bigtable_instance")
                .long("rpc-bigtable-instance")
                .value_name("INSTANCE_NAME")
                .takes_value(true)
                .hidden(hidden_unless_forced())
                .default_value("solana-ledger")
                .help("Name of BigTable instance to target"),
        )
        .arg(
            Arg::with_name("rpc_bigtable_app_profile_id")
                .long("rpc-bigtable-app-profile-id")
                .value_name("APP_PROFILE_ID")
                .takes_value(true)
                .hidden(hidden_unless_forced())
                .default_value(solana_storage_bigtable::DEFAULT_APP_PROFILE_ID)
                .help("Application profile id to use in Bigtable requests")
        )
        .arg(
            Arg::with_name("rpc_pubsub_enable_vote_subscription")
                .long("rpc-pubsub-enable-vote-subscription")
                .takes_value(false)
                .help("Enable the unstable RPC PubSub `voteSubscribe` subscription"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_enable_block_subscription")
                .long("rpc-pubsub-enable-block-subscription")
                .takes_value(false)
                .help("Enable the unstable RPC PubSub `blockSubscribe` subscription"),
        )
        .arg(
            Arg::with_name("bpf_program")
                .long("bpf-program")
                .value_names(&["ADDRESS_OR_KEYPAIR", "SBF_PROGRAM.SO"])
                .takes_value(true)
                .number_of_values(2)
                .multiple(true)
                .help(
                    "Add a SBF program to the genesis configuration with upgrades disabled. \
                       If the ledger already exists then this parameter is silently ignored. \
                       First argument can be a pubkey string or path to a keypair",
                ),
        )
        .arg(
            Arg::with_name("upgradeable_program")
                .long("upgradeable-program")
                .value_names(&["ADDRESS_OR_KEYPAIR", "SBF_PROGRAM.SO", "UPGRADE_AUTHORITY"])
                .takes_value(true)
                .number_of_values(3)
                .multiple(true)
                .help(
                    "Add an upgradeable SBF program to the genesis configuration. \
                       If the ledger already exists then this parameter is silently ignored. \
                       First and third arguments can be a pubkey string or path to a keypair. \
                       Upgrade authority set to \"none\" disables upgrades",
                ),
        )
        .arg(
            Arg::with_name("account")
                .long("account")
                .value_names(&["ADDRESS", "DUMP.JSON"])
                .takes_value(true)
                .number_of_values(2)
                .allow_hyphen_values(true)
                .multiple(true)
                .help(
                    "Load an account from the provided JSON file (see `solana account --help` on how to dump \
                        an account to file). Files are searched for relatively to CWD and tests/fixtures. \
                        If ADDRESS is omitted via the `-` placeholder, the one in the file will be used. \
                        If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("account_dir")
                .long("account-dir")
                .value_name("DIRECTORY")
                .validator(|value| {
                    value
                        .parse::<PathBuf>()
                        .map_err(|err| format!("error parsing '{value}': {err}"))
                        .and_then(|path| {
                            if path.exists() && path.is_dir() {
                                Ok(())
                            } else {
                                Err(format!("path does not exist or is not a directory: {value}"))
                            }
                        })
                })
                .takes_value(true)
                .multiple(true)
                .help(
                    "Load all the accounts from the JSON files found in the specified DIRECTORY \
                        (see also the `--account` flag). \
                        If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("ticks_per_slot")
                .long("ticks-per-slot")
                .value_name("TICKS")
                .validator(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|err| format!("error parsing '{value}': {err}"))
                        .and_then(|ticks| {
                            if ticks < MINIMUM_TICKS_PER_SLOT {
                                Err(format!("value must be >= {MINIMUM_TICKS_PER_SLOT}"))
                            } else {
                                Ok(())
                            }
                        })
                })
                .takes_value(true)
                .help("The number of ticks in a slot"),
        )
        .arg(
            Arg::with_name("slots_per_epoch")
                .long("slots-per-epoch")
                .value_name("SLOTS")
                .validator(|value| {
                    value
                        .parse::<Slot>()
                        .map_err(|err| format!("error parsing '{value}': {err}"))
                        .and_then(|slot| {
                            if slot < MINIMUM_SLOTS_PER_EPOCH {
                                Err(format!("value must be >= {MINIMUM_SLOTS_PER_EPOCH}"))
                            } else {
                                Ok(())
                            }
                        })
                })
                .takes_value(true)
                .help(
                    "Override the number of slots in an epoch. \
                       If the ledger already exists then this parameter is silently ignored",
                ),
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
                .help(
                    "Gossip DNS name or IP address for the validator to advertise in gossip \
                       [default: 127.0.0.1]",
                ),
        )
        .arg(
            Arg::with_name("dynamic_port_range")
                .long("dynamic-port-range")
                .value_name("MIN_PORT-MAX_PORT")
                .takes_value(true)
                .validator(port_range_validator)
                .help(
                    "Range to use for dynamically assigned ports \
                    [default: 1024-65535]",
                ),
        )
        .arg(
            Arg::with_name("bind_address")
                .long("bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .default_value("0.0.0.0")
                .help("IP address to bind the validator ports [default: 0.0.0.0]"),
        )
        .arg(
            Arg::with_name("clone_account")
                .long("clone")
                .short("c")
                .value_name("ADDRESS")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .multiple(true)
                .requires("json_rpc_url")
                .help(
                    "Copy an account from the cluster referenced by the --url argument the \
                     genesis configuration. \
                     If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("maybe_clone_account")
                .long("maybe-clone")
                .value_name("ADDRESS")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .multiple(true)
                .requires("json_rpc_url")
                .help(
                    "Copy an account from the cluster referenced by the --url argument, \
                     skipping it if it doesn't exist. \
                     If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("clone_upgradeable_program")
                .long("clone-upgradeable-program")
                .value_name("ADDRESS")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .multiple(true)
                .requires("json_rpc_url")
                .help(
                    "Copy an upgradeable program and its executable data from the cluster \
                     referenced by the --url argument the genesis configuration. \
                     If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("warp_slot")
                .required(false)
                .long("warp-slot")
                .short("w")
                .takes_value(true)
                .value_name("WARP_SLOT")
                .validator(is_slot)
                .min_values(0)
                .max_values(1)
                .help(
                    "Warp the ledger to WARP_SLOT after starting the validator. \
                        If no slot is provided then the current slot of the cluster \
                        referenced by the --url argument will be used",
                ),
        )
        .arg(
            Arg::with_name("limit_ledger_size")
                .long("limit-ledger-size")
                .value_name("SHRED_COUNT")
                .takes_value(true)
                .default_value(default_args.limit_ledger_size.as_str())
                .help("Keep this amount of shreds in root slots."),
        )
        .arg(
            Arg::with_name("faucet_sol")
                .long("faucet-sol")
                .takes_value(true)
                .value_name("SOL")
                .default_value(default_args.faucet_sol.as_str())
                .help(
                    "Give the faucet address this much SOL in genesis. \
                     If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("faucet_time_slice_secs")
                .long("faucet-time-slice-secs")
                .takes_value(true)
                .value_name("SECS")
                .default_value(default_args.faucet_time_slice_secs.as_str())
                .help(
                    "Time slice (in secs) over which to limit faucet requests",
                ),
        )
        .arg(
            Arg::with_name("faucet_per_time_sol_cap")
                .long("faucet-per-time-sol-cap")
                .takes_value(true)
                .value_name("SOL")
                .min_values(0)
                .max_values(1)
                .help(
                    "Per-time slice limit for faucet requests, in SOL",
                ),
        )
        .arg(
            Arg::with_name("faucet_per_request_sol_cap")
                .long("faucet-per-request-sol-cap")
                .takes_value(true)
                .value_name("SOL")
                .min_values(0)
                .max_values(1)
                .help(
                    "Per-request limit for faucet requests, in SOL",
                ),
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
            Arg::with_name("deactivate_feature")
                .long("deactivate-feature")
                .takes_value(true)
                .value_name("FEATURE_PUBKEY")
                .validator(is_pubkey)
                .multiple(true)
                .help("deactivate this feature in genesis.")
        )
        .arg(
            Arg::with_name("compute_unit_limit")
                .long("compute-unit-limit")
                .alias("max-compute-units")
                .value_name("COMPUTE_UNITS")
                .validator(is_parsable::<u64>)
                .takes_value(true)
                .help("Override the runtime's compute unit limit per transaction")
        )
        .arg(
            Arg::with_name("log_messages_bytes_limit")
                .long("log-messages-bytes-limit")
                .value_name("BYTES")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .help("Maximum number of bytes written to the program log before truncation")
        )
        .arg(
            Arg::with_name("transaction_account_lock_limit")
                .long("transaction-account-lock-limit")
                .value_name("NUM_ACCOUNTS")
                .validator(is_parsable::<u64>)
                .takes_value(true)
                .help("Override the runtime's account lock limit per transaction")
        );
}

pub struct DefaultTestArgs {
    pub rpc_port: String,
    pub faucet_port: String,
    pub limit_ledger_size: String,
    pub faucet_sol: String,
    pub faucet_time_slice_secs: String,
}

impl DefaultTestArgs {
    pub fn new() -> Self {
        DefaultTestArgs {
            rpc_port: rpc_port::DEFAULT_RPC_PORT.to_string(),
            faucet_port: FAUCET_PORT.to_string(),
            /* 10,000 was derived empirically by watching the size
             * of the rocksdb/ directory self-limit itself to the
             * 40MB-150MB range when running `solana-test-validator`
             */
            limit_ledger_size: 10_000.to_string(),
            faucet_sol: (1_000_000.).to_string(),
            faucet_time_slice_secs: (faucet::TIME_SLICE).to_string(),
        }
    }
}

impl Default for DefaultTestArgs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn make_sure_deprecated_arguments_are_sorted_alphabetically() {
        let deprecated = deprecated_arguments();

        for i in 0..deprecated.len().saturating_sub(1) {
            let curr_name = deprecated[i].arg.b.name;
            let next_name = deprecated[i + 1].arg.b.name;

            assert!(
                curr_name != next_name,
                "Arguments in `deprecated_arguments()` should be distinct.\n\
                 Arguments {} and {} use the same name: {}",
                i,
                i + 1,
                curr_name,
            );

            assert!(
                curr_name < next_name,
                "To generate better diffs and for readability purposes, `deprecated_arguments()` \
                 should list arguments in alphabetical order.\n\
                 Arguments {} and {} are not.\n\
                 Argument {} name: {}\n\
                 Argument {} name: {}",
                i,
                i + 1,
                i,
                curr_name,
                i + 1,
                next_name,
            );
        }
    }
}
