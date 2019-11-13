use clap::{crate_description, crate_name, crate_version, value_t, App, Arg, ArgMatches};
use solana_core::gen_keys::GenKeys;
use solana_drone::drone::DRONE_PORT;
use solana_sdk::signature::{read_keypair_file, Keypair, KeypairUtil};
use std::net::SocketAddr;
use std::process::exit;
use std::time::Duration;

pub struct Config {
    pub entrypoint_addr: SocketAddr,
    pub drone_addr: SocketAddr,
    pub identity: Keypair,
    pub threads: usize,
    pub num_nodes: usize,
    pub duration: Duration,
    pub transfer_delay: u64,
    pub fund_amount: u64,
    pub batch_size: usize,
    pub chunk_size: usize,
    pub account_groups: usize,
    pub client_ids_and_stake_file: String,
    pub write_to_client_file: bool,
    pub read_from_client_file: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            entrypoint_addr: SocketAddr::from(([127, 0, 0, 1], 8001)),
            drone_addr: SocketAddr::from(([127, 0, 0, 1], DRONE_PORT)),
            identity: Keypair::new(),
            num_nodes: 1,
            threads: 4,
            duration: Duration::new(u64::max_value(), 0),
            transfer_delay: 0,
            fund_amount: 100_000,
            batch_size: 100,
            chunk_size: 100,
            account_groups: 100,
            client_ids_and_stake_file: String::new(),
            write_to_client_file: false,
            read_from_client_file: false,
        }
    }
}

pub fn build_args<'a, 'b>() -> App<'a, 'b> {
    App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .required(false)
                .default_value("127.0.0.1:8001")
                .help("Cluster entry point; defaults to 127.0.0.1:8001"),
        )
        .arg(
            Arg::with_name("drone")
                .short("d")
                .long("drone")
                .value_name("HOST:PORT")
                .takes_value(true)
                .required(false)
                .default_value("127.0.0.1:9900")
                .help("Location of the drone; defaults to 127.0.0.1:9900"),
        )
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("File containing a client identity (keypair)"),
        )
        .arg(
            Arg::with_name("threads")
                .long("threads")
                .value_name("<threads>")
                .takes_value(true)
                .required(false)
                .default_value("1")
                .help("Number of threads submitting transactions"),
        )
        .arg(
            Arg::with_name("num-nodes")
                .long("num-nodes")
                .value_name("NUM")
                .takes_value(true)
                .required(false)
                .default_value("1")
                .help("Wait for NUM nodes to converge"),
        )
        .arg(
            Arg::with_name("duration")
                .long("duration")
                .value_name("SECS")
                .takes_value(true)
                .default_value("60")
                .help("Seconds to run benchmark, then exit; default is forever"),
        )
        .arg(
            Arg::with_name("transfer-delay")
                .long("transfer-delay")
                .value_name("<delay>")
                .takes_value(true)
                .required(false)
                .default_value("0")
                .help("Delay between each chunk"),
        )
        .arg(
            Arg::with_name("fund-amount")
                .long("fund-amount")
                .value_name("<fund>")
                .takes_value(true)
                .required(false)
                .default_value("100000")
                .help("Number of lamports to fund to each signer"),
        )
        .arg(
            Arg::with_name("batch-size")
                .long("batch-size")
                .value_name("<batch>")
                .takes_value(true)
                .required(false)
                .default_value("1000")
                .help("Number of transactions before the signer rolls over"),
        )
        .arg(
            Arg::with_name("chunk-size")
                .long("chunk-size")
                .value_name("<cunk>")
                .takes_value(true)
                .required(false)
                .default_value("500")
                .help("Number of transactions to generate and send at a time"),
        )
        .arg(
            Arg::with_name("account-groups")
                .long("account-groups")
                .value_name("<groups>")
                .takes_value(true)
                .required(false)
                .default_value("10")
                .help("Number of account groups to cycle for each batch"),
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
}

pub fn extract_args<'a>(matches: &ArgMatches<'a>) -> Config {
    let mut args = Config::default();

    args.entrypoint_addr = solana_net_utils::parse_host_port(
        matches.value_of("entrypoint").unwrap(),
    )
    .unwrap_or_else(|e| {
        eprintln!("failed to parse entrypoint address: {}", e);
        exit(1)
    });

    args.drone_addr = solana_net_utils::parse_host_port(matches.value_of("drone").unwrap())
        .unwrap_or_else(|e| {
            eprintln!("failed to parse drone address: {}", e);
            exit(1)
        });

    if matches.is_present("identity") {
        args.identity = read_keypair_file(matches.value_of("identity").unwrap())
            .expect("can't read client identity");
    } else {
        args.identity = {
            let seed = [42_u8; 32];
            let mut rnd = GenKeys::new(seed);
            rnd.gen_keypair()
        };
    }
    args.threads = value_t!(matches.value_of("threads"), usize).expect("Failed to parse threads");
    args.num_nodes =
        value_t!(matches.value_of("num-nodes"), usize).expect("Failed to parse num-nodes");
    let duration = value_t!(matches.value_of("duration"), u64).expect("Failed to parse duration");
    args.duration = Duration::from_secs(duration);
    args.transfer_delay =
        value_t!(matches.value_of("transfer-delay"), u64).expect("Failed to parse transfer-delay");
    args.fund_amount =
        value_t!(matches.value_of("fund-amount"), u64).expect("Failed to parse fund-amount");
    args.batch_size =
        value_t!(matches.value_of("batch-size"), usize).expect("Failed to parse batch-size");
    args.chunk_size =
        value_t!(matches.value_of("chunk-size"), usize).expect("Failed to parse chunk-size");
    args.account_groups = value_t!(matches.value_of("account-groups"), usize)
        .expect("Failed to parse account-groups");

    if let Some(s) = matches.value_of("write-client-keys") {
        args.write_to_client_file = true;
        args.client_ids_and_stake_file = s.to_string();
    }

    if let Some(s) = matches.value_of("read-client-keys") {
        assert!(!args.write_to_client_file);
        args.read_from_client_file = true;
        args.client_ids_and_stake_file = s.to_string();
    }
    args
}
