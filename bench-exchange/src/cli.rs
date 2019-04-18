use std::net::SocketAddr;
use std::time::Duration;

use clap::{crate_description, crate_name, crate_version, value_t, App, Arg, ArgMatches};
use solana_drone::drone::DRONE_PORT;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};
use untrusted::Input;

pub struct Config {
    pub network_addr: SocketAddr,
    pub drone_addr: SocketAddr,
    pub identity: Keypair,
    pub threads: usize,
    pub num_nodes: usize,
    pub duration: Duration,
    pub trade_delay: u64,
    pub fund_amount: u64,
    pub batch_size: usize,
    pub account_groups: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network_addr: SocketAddr::from(([127, 0, 0, 1], 8001)),
            drone_addr: SocketAddr::from(([127, 0, 0, 1], DRONE_PORT)),
            identity: Keypair::new(),
            num_nodes: 1,
            threads: 4,
            duration: Duration::new(u64::max_value(), 0),
            trade_delay: 0,
            fund_amount: 100_000,
            batch_size: 100,
            account_groups: 100,
        }
    }
}

pub fn build_args<'a, 'b>() -> App<'a, 'b> {
    App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .required(false)
                .default_value("127.0.0.1:8001")
                .help("Network's gossip entry point; defaults to 127.0.0.1:8001"),
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
                .default_value("4")
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
            Arg::with_name("trade-delay")
                .long("trade-delay")
                .value_name("<delay>")
                .takes_value(true)
                .required(false)
                .default_value("0")
                .help("Delay between trade requests in milliseconds"),
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
                .help("Number of bulk trades to submit between trade delays"),
        )
        .arg(
            Arg::with_name("account-groups")
                .long("account-groups")
                .value_name("<groups>")
                .takes_value(true)
                .required(false)
                .default_value("100")
                .help("Number of account groups to cycle for each batch"),
        )
}

pub fn extract_args<'a>(matches: &ArgMatches<'a>) -> Config {
    let mut args = Config::default();

    args.network_addr = matches
        .value_of("network")
        .unwrap()
        .parse()
        .expect("Failed to parse network");
    args.drone_addr = matches
        .value_of("drone")
        .unwrap()
        .parse()
        .expect("Failed to parse drone address");

    if matches.is_present("identity") {
        args.identity = read_keypair(matches.value_of("identity").unwrap())
            .expect("can't read client identity");
    } else {
        args.identity = {
            let seed = [42_u8; 32];
            Keypair::from_seed_unchecked(Input::from(&seed)).unwrap()
        };
    }
    args.threads = value_t!(matches.value_of("threads"), usize).expect("Failed to parse threads");
    args.num_nodes =
        value_t!(matches.value_of("num-nodes"), usize).expect("Failed to parse num-nodes");
    let duration = value_t!(matches.value_of("duration"), u64).expect("Failed to parse duration");
    args.duration = Duration::from_secs(duration);
    args.trade_delay =
        value_t!(matches.value_of("trade-delay"), u64).expect("Failed to parse trade-delay");
    args.fund_amount =
        value_t!(matches.value_of("fund-amount"), u64).expect("Failed to parse fund-amount");
    args.batch_size =
        value_t!(matches.value_of("batch-size"), usize).expect("Failed to parse batch-size");
    args.account_groups = value_t!(matches.value_of("account-groups"), usize)
        .expect("Failed to parse account-groups");

    args
}
