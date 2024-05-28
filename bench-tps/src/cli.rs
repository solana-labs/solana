use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg, ArgMatches},
    solana_clap_utils::{
        hidden_unless_forced,
        input_validators::{is_keypair, is_url, is_url_or_moniker, is_within_range},
    },
    solana_cli_config::{ConfigInput, CONFIG_FILE},
    solana_sdk::{
        commitment_config::CommitmentConfig,
        fee_calculator::FeeRateGovernor,
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair},
    },
    solana_streamer::nonblocking::quic::DEFAULT_MAX_CONNECTIONS_PER_IPADDR_PER_MINUTE,
    solana_tpu_client::tpu_client::{DEFAULT_TPU_CONNECTION_POOL_SIZE, DEFAULT_TPU_USE_QUIC},
    std::{
        net::{IpAddr, Ipv4Addr},
        time::Duration,
    },
};

const NUM_LAMPORTS_PER_ACCOUNT_DEFAULT: u64 = solana_sdk::native_token::LAMPORTS_PER_SOL;

#[derive(Eq, PartialEq, Debug)]
pub enum ExternalClientType {
    // Submits transactions to an Rpc node using an RpcClient
    RpcClient,
    // Submits transactions directly to leaders using a TpuClient, broadcasting to upcoming leaders
    // via TpuClient default configuration
    TpuClient,
}

impl Default for ExternalClientType {
    fn default() -> Self {
        Self::TpuClient
    }
}

#[derive(Eq, PartialEq, Debug)]
pub struct InstructionPaddingConfig {
    pub program_id: Pubkey,
    pub data_size: u32,
}

#[derive(Debug, PartialEq)]
pub enum ComputeUnitPrice {
    Fixed(u64),
    Random,
}

/// Holds the configuration for a single run of the benchmark
#[derive(PartialEq, Debug)]
pub struct Config {
    pub json_rpc_url: String,
    pub websocket_url: String,
    pub id: Keypair,
    pub threads: usize,
    pub duration: Duration,
    pub tx_count: usize,
    pub keypair_multiplier: usize,
    pub thread_batch_sleep_ms: usize,
    pub sustained: bool,
    pub client_ids_and_stake_file: String,
    pub write_to_client_file: bool,
    pub read_from_client_file: bool,
    pub target_lamports_per_signature: u64,
    pub num_lamports_per_account: u64,
    pub target_slots_per_epoch: u64,
    pub external_client_type: ExternalClientType,
    pub use_quic: bool,
    pub tpu_connection_pool_size: usize,
    pub tpu_max_connections_per_ipaddr_per_minute: u64,
    pub compute_unit_price: Option<ComputeUnitPrice>,
    pub skip_tx_account_data_size: bool,
    pub use_durable_nonce: bool,
    pub instruction_padding_config: Option<InstructionPaddingConfig>,
    pub num_conflict_groups: Option<usize>,
    pub bind_address: IpAddr,
    pub client_node_id: Option<Keypair>,
    pub commitment_config: CommitmentConfig,
    pub block_data_file: Option<String>,
    pub transaction_data_file: Option<String>,
}

impl Eq for Config {}

impl Default for Config {
    fn default() -> Config {
        Config {
            json_rpc_url: ConfigInput::default().json_rpc_url,
            websocket_url: ConfigInput::default().websocket_url,
            id: Keypair::new(),
            threads: 4,
            duration: Duration::new(u64::MAX, 0),
            tx_count: 50_000,
            keypair_multiplier: 8,
            thread_batch_sleep_ms: 1000,
            sustained: false,
            client_ids_and_stake_file: String::new(),
            write_to_client_file: false,
            read_from_client_file: false,
            target_lamports_per_signature: FeeRateGovernor::default().target_lamports_per_signature,
            num_lamports_per_account: NUM_LAMPORTS_PER_ACCOUNT_DEFAULT,
            target_slots_per_epoch: 0,
            external_client_type: ExternalClientType::default(),
            use_quic: DEFAULT_TPU_USE_QUIC,
            tpu_connection_pool_size: DEFAULT_TPU_CONNECTION_POOL_SIZE,
            tpu_max_connections_per_ipaddr_per_minute:
                DEFAULT_MAX_CONNECTIONS_PER_IPADDR_PER_MINUTE,
            compute_unit_price: None,
            skip_tx_account_data_size: false,
            use_durable_nonce: false,
            instruction_padding_config: None,
            num_conflict_groups: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            client_node_id: None,
            commitment_config: CommitmentConfig::confirmed(),
            block_data_file: None,
            transaction_data_file: None,
        }
    }
}

/// Defines and builds the CLI args for a run of the benchmark
pub fn build_args<'a>(version: &'_ str) -> App<'a, '_> {
    App::new(crate_name!()).about(crate_description!())
        .version(version)
        .arg({
            let arg = Arg::with_name("config_file")
                .short("C")
                .long("config")
                .value_name("FILEPATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *CONFIG_FILE {
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
                .global(true)
                .validator(is_url_or_moniker)
                .help(
                    "URL for Solana's JSON RPC or moniker (or their first letter): \
                       [mainnet-beta, testnet, devnet, localhost]",
                ),
        )
        .arg(
            Arg::with_name("websocket_url")
                .long("ws")
                .value_name("URL")
                .takes_value(true)
                .global(true)
                .validator(is_url)
                .help("WebSocket URL for the solana cluster"),
        )
        .arg(
            Arg::with_name("rpc_addr")
                .long("rpc-addr")
                .value_name("HOST:PORT")
                .takes_value(true)
                .conflicts_with("rpc_client")
                .requires("tpu_addr")
                .hidden(hidden_unless_forced())
                .help("Specify custom rpc_addr to create thin_client. \
                    Note: ThinClient is deprecated. Argument will not be used. \
                    Use tpc_client or rpc_client instead"),
        )
        .arg(
            Arg::with_name("tpu_addr")
                .long("tpu-addr")
                .value_name("HOST:PORT")
                .conflicts_with("rpc_client")
                .takes_value(true)
                .requires("rpc_addr")
                .hidden(hidden_unless_forced())
                .help("Specify custom tpu_addr to create thin_client. \
                    Note: ThinClient is deprecated. Argument will not be used. \
                    Use tpc_client or rpc_client instead"),
        )
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .hidden(hidden_unless_forced())
                .help("Rendezvous with the cluster at this entry point; defaults to 127.0.0.1:8001. \
                    Note: ThinClient is deprecated. Argument will not be used. \
                    Use tpc_client or rpc_client instead"),
        )
        .arg(
            Arg::with_name("faucet")
                .short("d")
                .long("faucet")
                .value_name("HOST:PORT")
                .takes_value(true)
                .hidden(hidden_unless_forced())
                .help("Deprecated. BenchTps no longer queries the faucet directly"),
        )
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .hidden(hidden_unless_forced())
                .help("Deprecated. Use --authority instead"),

        )
        .arg(
            Arg::with_name("authority")
                .short("a")
                .long("authority")
                .value_name("PATH")
                .takes_value(true)
                .help("File containing a client authority (keypair) to fund participating accounts"),
        )
        .arg(
            Arg::with_name("num-nodes")
                .short("N")
                .long("num-nodes")
                .value_name("NUM")
                .takes_value(true)
                .hidden(hidden_unless_forced())
                .help("Wait for NUM nodes to converge. \
                    Note: ThinClient is deprecated. Argument will not be used. \
                    Use tpc_client or rpc_client instead"),
        )
        .arg(
            Arg::with_name("threads")
                .short("t")
                .long("threads")
                .value_name("NUM")
                .takes_value(true)
                .help("Number of threads"),
        )
        .arg(
            Arg::with_name("duration")
                .long("duration")
                .value_name("SECS")
                .takes_value(true)
                .help("Seconds to run benchmark, then exit; default is forever"),
        )
        .arg(
            Arg::with_name("sustained")
                .long("sustained")
                .help("Use sustained performance mode vs. peak mode. This overlaps the tx generation with transfers."),
        )
        .arg(
            Arg::with_name("no-multi-client")
                .long("no-multi-client")
                .hidden(hidden_unless_forced())
                .help("Disable multi-client support, only transact with the entrypoint. \
                    Note: ThinClient is deprecated. Flag will not be used. \
                    Use tpc_client or rpc_client instead"),
        )
        .arg(
            Arg::with_name("target_node")
                .long("target-node")
                .requires("no-multi-client")
                .takes_value(true)
                .value_name("PUBKEY")
                .hidden(hidden_unless_forced())
                .help("Specify an exact node to send transactions to. \
                    Note: ThinClient is deprecated. Argument will not be used. \
                    Use tpc_client or rpc_client instead"),
        )
        .arg(
            Arg::with_name("tx_count")
                .long("tx-count")
                .alias("tx_count")
                .value_name("NUM")
                .takes_value(true)
                .help("Number of transactions to send per batch")
        )
        .arg(
            Arg::with_name("keypair_multiplier")
                .long("keypair-multiplier")
                .value_name("NUM")
                .takes_value(true)
                .help("Multiply by transaction count to determine number of keypairs to create")
        )
        .arg(
            Arg::with_name("thread-batch-sleep-ms")
                .short("z")
                .long("thread-batch-sleep-ms")
                .value_name("NUM")
                .takes_value(true)
                .help("Per-thread-per-iteration sleep in ms"),
        )
        .arg(
            Arg::with_name("write-client-keys")
                .long("write-client-keys")
                .value_name("FILENAME")
                .takes_value(true)
                .help("Generate client keys and stakes and write the list to YAML file"),
        )
        .arg(
            Arg::with_name("read-client-keys")
                .long("read-client-keys")
                .value_name("FILENAME")
                .takes_value(true)
                .help("Read client keys and stakes from the YAML file"),
        )
        .arg(
            Arg::with_name("target_lamports_per_signature")
                .long("target-lamports-per-signature")
                .value_name("LAMPORTS")
                .takes_value(true)
                .help(
                    "The cost in lamports that the cluster will charge for signature \
                     verification when the cluster is operating at target-signatures-per-slot",
                ),
        )
        .arg(
            Arg::with_name("num_lamports_per_account")
                .long("num-lamports-per-account")
                .value_name("LAMPORTS")
                .takes_value(true)
                .help(
                    "Number of lamports per account.",
                ),
        )
        .arg(
            Arg::with_name("target_slots_per_epoch")
                .long("target-slots-per-epoch")
                .value_name("SLOTS")
                .takes_value(true)
                .help(
                    "Wait until epochs are this many slots long.",
                ),
        )
        .arg(
            Arg::with_name("rpc_client")
                .long("use-rpc-client")
                .conflicts_with("tpu_client")
                .takes_value(false)
                .help("Submit transactions with a RpcClient")
        )
        .arg(
            Arg::with_name("tpu_client")
                .long("use-tpu-client")
                .conflicts_with("rpc_client")
                .takes_value(false)
                .help("Submit transactions with a TpuClient")
        )
        .arg(
            Arg::with_name("tpu_disable_quic")
                .long("tpu-disable-quic")
                .takes_value(false)
                .help("Do not submit transactions via QUIC; only affects TpuClient (default) sends"),
        )
        .arg(
            Arg::with_name("tpu_connection_pool_size")
                .long("tpu-connection-pool-size")
                .takes_value(true)
                .help("Controls the connection pool size per remote address; only affects TpuClient (default) sends"),
        )
        .arg(
            Arg::with_name("compute_unit_price")
            .long("compute-unit-price")
            .takes_value(true)
            .validator(|s| is_within_range(s, 0..))
            .help("Sets constant compute-unit-price to transfer transactions"),
        )
        .arg(
            Arg::with_name("use_randomized_compute_unit_price")
                .long("use-randomized-compute-unit-price")
                .takes_value(false)
                .conflicts_with("compute_unit_price")
                .help("Sets random compute-unit-price in range [0..100] to transfer transactions"),
        )
        .arg(
            Arg::with_name("skip_tx_account_data_size")
                .long("skip-tx-account-data-size")
                .takes_value(false)
                .conflicts_with("instruction_padding_data_size")
                .help("Skips setting the account data size for each transaction"),
        )
        .arg(
            Arg::with_name("use_durable_nonce")
                .long("use-durable-nonce")
                .help("Use durable transaction nonce instead of recent blockhash"),
        )
        .arg(
            Arg::with_name("instruction_padding_program_id")
                .long("instruction-padding-program-id")
                .requires("instruction_padding_data_size")
                .takes_value(true)
                .value_name("PUBKEY")
                .help("If instruction data is padded, optionally specify the padding program id to target"),
        )
        .arg(
            Arg::with_name("instruction_padding_data_size")
                .long("instruction-padding-data-size")
                .takes_value(true)
                .help("If set, wraps all instructions in the instruction padding program, with the given amount of padding bytes in instruction data."),
        )
        .arg(
            Arg::with_name("num_conflict_groups")
                .long("num-conflict-groups")
                .takes_value(true)
                .validator(|arg| is_within_range(arg, 1..))
                .help("The number of unique destination accounts per transactions 'chunk'. Lower values will result in more transaction conflicts.")
        )
        .arg(
            Arg::with_name("bind_address")
                .long("bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .requires("client_node_id")
                .help("IP address to use with connection cache"),
        )
        .arg(
            Arg::with_name("client_node_id")
                .long("client-node-id")
                .value_name("PATH")
                .takes_value(true)
                .requires("json_rpc_url")
                .validator(is_keypair)
                .help("File containing the node identity (keypair) of a validator with active stake. This allows communicating with network using staked connection"),
        )
        .arg(
            Arg::with_name("commitment_config")
                .long("commitment-config")
                .takes_value(true)
                .possible_values(&["processed", "confirmed", "finalized"])
                .default_value("confirmed")
                .help("Block commitment config for getting latest blockhash"),
        )
        .arg(
            Arg::with_name("block_data_file")
                .long("block-data-file")
                .value_name("FILENAME")
                .takes_value(true)
                .help("File to save block statistics relevant to the submitted transactions."),
        )
        .arg(
            Arg::with_name("transaction_data_file")
                .long("transaction-data-file")
                .value_name("FILENAME")
                .takes_value(true)
                .help(
                    "File to save details about all the submitted transactions.\
                    This option is useful for debug purposes."
                ),
        )
}

/// Parses a clap `ArgMatches` structure into a `Config`
pub fn parse_args(matches: &ArgMatches) -> Result<Config, &'static str> {
    let mut args = Config::default();

    let config = if let Some(config_file) = matches.value_of("config_file") {
        solana_cli_config::Config::load(config_file).unwrap_or_default()
    } else {
        solana_cli_config::Config::default()
    };
    let (_, json_rpc_url) = ConfigInput::compute_json_rpc_url_setting(
        matches.value_of("json_rpc_url").unwrap_or(""),
        &config.json_rpc_url,
    );
    args.json_rpc_url = json_rpc_url;

    let (_, websocket_url) = ConfigInput::compute_websocket_url_setting(
        matches.value_of("websocket_url").unwrap_or(""),
        &config.websocket_url,
        matches.value_of("json_rpc_url").unwrap_or(""),
        &config.json_rpc_url,
    );
    args.websocket_url = websocket_url;

    if matches.is_present("identity") {
        eprintln!("Warning: --identity is deprecated. Please use --authority");
    }

    let (_, id_path) = ConfigInput::compute_keypair_path_setting(
        matches
            .value_of("authority")
            .or(matches.value_of("identity"))
            .unwrap_or(""),
        &config.keypair_path,
    );
    if let Ok(id) = read_keypair_file(id_path) {
        args.id = id;
    } else if matches.is_present("identity") || matches.is_present("authority") {
        return Err("could not parse authority path");
    }

    if matches.is_present("rpc_client") {
        args.external_client_type = ExternalClientType::RpcClient;
    }

    if matches.is_present("tpu_disable_quic") {
        args.use_quic = false;
    }

    if let Some(v) = matches.value_of("tpu_connection_pool_size") {
        args.tpu_connection_pool_size = v
            .to_string()
            .parse::<usize>()
            .map_err(|_| "can't parse tpu-connection-pool-size")?;
    }

    if let Some(t) = matches.value_of("threads") {
        args.threads = t.to_string().parse().map_err(|_| "can't parse threads")?;
    }

    if let Some(duration) = matches.value_of("duration") {
        let seconds = duration
            .to_string()
            .parse()
            .map_err(|_| "can't parse duration")?;
        args.duration = Duration::new(seconds, 0);
    }

    if let Some(s) = matches.value_of("tx_count") {
        args.tx_count = s.to_string().parse().map_err(|_| "can't parse tx_count")?;
    }

    if let Some(s) = matches.value_of("keypair_multiplier") {
        args.keypair_multiplier = s
            .to_string()
            .parse()
            .map_err(|_| "can't parse keypair-multiplier")?;
        if args.keypair_multiplier < 2 {
            return Err("args.keypair_multiplier must be greater than or equal to 2");
        }
    }

    if let Some(t) = matches.value_of("thread-batch-sleep-ms") {
        args.thread_batch_sleep_ms = t
            .to_string()
            .parse()
            .map_err(|_| "can't parse thread-batch-sleep-ms")?;
    }

    args.sustained = matches.is_present("sustained");

    if let Some(s) = matches.value_of("write-client-keys") {
        args.write_to_client_file = true;
        args.client_ids_and_stake_file = s.to_string();
    }

    if let Some(s) = matches.value_of("read-client-keys") {
        assert!(!args.write_to_client_file);
        args.read_from_client_file = true;
        args.client_ids_and_stake_file = s.to_string();
    }

    if let Some(v) = matches.value_of("target_lamports_per_signature") {
        args.target_lamports_per_signature = v
            .to_string()
            .parse()
            .map_err(|_| "can't parse target-lamports-per-signature")?;
    }

    if let Some(v) = matches.value_of("num_lamports_per_account") {
        args.num_lamports_per_account = v
            .to_string()
            .parse()
            .map_err(|_| "can't parse num-lamports-per-account")?;
    }

    if let Some(t) = matches.value_of("target_slots_per_epoch") {
        args.target_slots_per_epoch = t
            .to_string()
            .parse()
            .map_err(|_| "can't parse target-slots-per-epoch")?;
    }

    if let Some(str) = matches.value_of("compute_unit_price") {
        args.compute_unit_price = Some(ComputeUnitPrice::Fixed(
            str.parse().map_err(|_| "can't parse compute-unit-price")?,
        ));
    }

    if matches.is_present("use_randomized_compute_unit_price") {
        args.compute_unit_price = Some(ComputeUnitPrice::Random);
    }

    if matches.is_present("skip_tx_account_data_size") {
        args.skip_tx_account_data_size = true;
    }

    if matches.is_present("use_durable_nonce") {
        args.use_durable_nonce = true;
    }

    if let Some(data_size) = matches.value_of("instruction_padding_data_size") {
        let program_id = matches
            .value_of("instruction_padding_program_id")
            .map(|target_str| target_str.parse().unwrap())
            .unwrap_or_else(|| spl_instruction_padding::ID);
        let data_size = data_size
            .parse()
            .map_err(|_| "Can't parse padded instruction data size")?;
        args.instruction_padding_config = Some(InstructionPaddingConfig {
            program_id,
            data_size,
        });
    }

    if let Some(num_conflict_groups) = matches.value_of("num_conflict_groups") {
        let parsed_num_conflict_groups = num_conflict_groups
            .parse()
            .map_err(|_| "Can't parse num-conflict-groups")?;
        args.num_conflict_groups = Some(parsed_num_conflict_groups);
    }

    if let Some(addr) = matches.value_of("bind_address") {
        args.bind_address =
            solana_net_utils::parse_host(addr).map_err(|_| "Failed to parse bind-address")?;
    }

    if let Some(client_node_id_filename) = matches.value_of("client_node_id") {
        // error is checked by arg validator
        let client_node_id = read_keypair_file(client_node_id_filename).map_err(|_| "")?;
        args.client_node_id = Some(client_node_id);
    }

    args.commitment_config = value_t_or_exit!(matches, "commitment_config", CommitmentConfig);
    args.block_data_file = matches.value_of("block_data_file").map(|s| s.to_string());
    args.transaction_data_file = matches
        .value_of("transaction_data_file")
        .map(|s| s.to_string());

    Ok(args)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::signature::{read_keypair_file, write_keypair_file, Keypair, Signer},
        std::{
            net::{IpAddr, Ipv4Addr},
            time::Duration,
        },
        tempfile::{tempdir, TempDir},
    };

    /// create a keypair and write it to json file in temporary directory
    /// return both generated keypair and full file name
    fn write_tmp_keypair(out_dir: &TempDir) -> (Keypair, String) {
        let keypair = Keypair::new();
        let file_path = out_dir
            .path()
            .join(format!("keypair_file-{}", keypair.pubkey()));
        let keypair_file_name = file_path.into_os_string().into_string().unwrap();
        write_keypair_file(&keypair, &keypair_file_name).unwrap();
        (keypair, keypair_file_name)
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse() {
        // create a directory inside of std::env::temp_dir(), removed when out_dir goes out of scope
        let out_dir = tempdir().unwrap();
        let (keypair, keypair_file_name) = write_tmp_keypair(&out_dir);

        // parse provided rpc address, check that default ws address is correct
        // always specify authority in these tests because otherwise a random one will be used
        let matches = build_args("1.0.0").get_matches_from(vec![
            "solana-bench-tps",
            "--authority",
            &keypair_file_name,
            "-u",
            "http://123.4.5.6:8899",
        ]);
        let actual = parse_args(&matches).unwrap();
        assert_eq!(
            actual,
            Config {
                json_rpc_url: "http://123.4.5.6:8899".to_string(),
                websocket_url: "ws://123.4.5.6:8900/".to_string(),
                id: keypair,
                ..Config::default()
            }
        );

        // check if --identity is working
        let keypair = read_keypair_file(&keypair_file_name).unwrap();
        let matches = build_args("1.0.0").get_matches_from(vec![
            "solana-bench-tps",
            "--identity",
            &keypair_file_name,
            "-u",
            "http://123.4.5.6:8899",
        ]);
        let actual = parse_args(&matches).unwrap();
        assert_eq!(
            actual,
            Config {
                json_rpc_url: "http://123.4.5.6:8899".to_string(),
                websocket_url: "ws://123.4.5.6:8900/".to_string(),
                id: keypair,
                ..Config::default()
            }
        );

        // parse cli args typical for private cluster tests
        let keypair = read_keypair_file(&keypair_file_name).unwrap();
        let matches = build_args("1.0.0").get_matches_from(vec![
            "solana-bench-tps",
            "--authority",
            &keypair_file_name,
            "-u",
            "http://123.4.5.6:8899",
            "--duration",
            "1000",
            "--sustained",
            "--threads",
            "4",
            "--read-client-keys",
            "./client-accounts.yml",
        ]);
        let actual = parse_args(&matches).unwrap();
        assert_eq!(
            actual,
            Config {
                json_rpc_url: "http://123.4.5.6:8899".to_string(),
                websocket_url: "ws://123.4.5.6:8900/".to_string(),
                id: keypair,
                duration: Duration::new(1000, 0),
                sustained: true,
                threads: 4,
                read_from_client_file: true,
                client_ids_and_stake_file: "./client-accounts.yml".to_string(),
                ..Config::default()
            }
        );

        // select different client type and CommitmentConfig
        let keypair = read_keypair_file(&keypair_file_name).unwrap();
        let matches = build_args("1.0.0").get_matches_from(vec![
            "solana-bench-tps",
            "--authority",
            &keypair_file_name,
            "-u",
            "http://123.4.5.6:8899",
            "--use-rpc-client",
            "--commitment-config",
            "finalized",
        ]);
        let actual = parse_args(&matches).unwrap();
        assert_eq!(
            actual,
            Config {
                json_rpc_url: "http://123.4.5.6:8899".to_string(),
                websocket_url: "ws://123.4.5.6:8900/".to_string(),
                id: keypair,
                external_client_type: ExternalClientType::RpcClient,
                commitment_config: CommitmentConfig::finalized(),
                ..Config::default()
            }
        );

        // with client node id
        let keypair = read_keypair_file(&keypair_file_name).unwrap();
        let (client_id, client_id_file_name) = write_tmp_keypair(&out_dir);
        let matches = build_args("1.0.0").get_matches_from(vec![
            "solana-bench-tps",
            "--authority",
            &keypair_file_name,
            "-u",
            "http://192.0.0.1:8899",
            "--bind-address",
            "192.9.8.7",
            "--client-node-id",
            &client_id_file_name,
        ]);
        let actual = parse_args(&matches).unwrap();
        assert_eq!(
            actual,
            Config {
                json_rpc_url: "http://192.0.0.1:8899".to_string(),
                websocket_url: "ws://192.0.0.1:8900/".to_string(),
                id: keypair,
                bind_address: IpAddr::V4(Ipv4Addr::new(192, 9, 8, 7)),
                client_node_id: Some(client_id),
                ..Config::default()
            }
        );
    }
}
