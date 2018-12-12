use std::net::SocketAddr;
use std::process::exit;
use std::time::Duration;

use clap::{App, Arg, ArgMatches};
use solana_drone::drone::DRONE_PORT;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};

/// Holds the configuration for a single run of the benchmark
pub struct Config {
    pub network_addr: SocketAddr,
    pub drone_addr: SocketAddr,
    pub id: Keypair,
    pub threads: usize,
    pub num_nodes: usize,
    pub duration: Duration,
    pub tx_count: usize,
    pub sustained: bool,
    pub reject_extra_nodes: bool,
    pub converge_only: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            network_addr: SocketAddr::from(([127, 0, 0, 1], 8001)),
            drone_addr: SocketAddr::from(([127, 0, 0, 1], DRONE_PORT)),
            id: Keypair::new(),
            threads: 4,
            num_nodes: 1,
            duration: Duration::new(std::u64::MAX, 0),
            tx_count: 500_000,
            sustained: false,
            reject_extra_nodes: false,
            converge_only: false,
        }
    }
}

/// Defines and builds the CLI args for a run of the benchmark
pub fn build_args<'a, 'b>() -> App<'a, 'b> {
    App::new("solana-bench-tps")
        .version(crate_version!())
        .arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the network at this gossip entry point; defaults to 127.0.0.1:8001"),
        )
        .arg(
            Arg::with_name("drone")
                .short("d")
                .long("drone")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Location of the drone; defaults to network:DRONE_PORT"),
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
            Arg::with_name("num-nodes")
                .short("N")
                .long("num-nodes")
                .value_name("NUM")
                .takes_value(true)
                .help("Wait for NUM nodes to converge"),
        )
        .arg(
            Arg::with_name("reject-extra-nodes")
                .long("reject-extra-nodes")
                .help("Require exactly `num-nodes` on convergence. Appropriate only for internal networks"),
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
            Arg::with_name("converge-only")
                .long("converge-only")
                .help("Exit immediately after converging"),
        )
        .arg(
            Arg::with_name("sustained")
                .long("sustained")
                .help("Use sustained performance mode vs. peak mode. This overlaps the tx generation with transfers."),
        )
        .arg(
            Arg::with_name("tx_count")
                .long("tx_count")
                .value_name("NUM")
                .takes_value(true)
                .help("Number of transactions to send per batch")
        )
}

/// Parses a clap `ArgMatches` structure into a `Config`
/// # Arguments
/// * `matches` - command line arguments parsed by clap
/// # Panics
/// Panics if there is trouble parsing any of the arguments
pub fn extract_args<'a>(matches: &ArgMatches<'a>) -> Config {
    let mut args = Config::default();

    if let Some(addr) = matches.value_of("network") {
        args.network_addr = addr.parse().unwrap_or_else(|e| {
            eprintln!("failed to parse network: {}", e);
            exit(1)
        });
    }

    if let Some(addr) = matches.value_of("drone") {
        args.drone_addr = addr.parse().unwrap_or_else(|e| {
            eprintln!("failed to parse drone address: {}", e);
            exit(1)
        });
    }

    if matches.is_present("identity") {
        args.id = read_keypair(matches.value_of("identity").unwrap())
            .expect("can't read client identity");
    }

    if let Some(t) = matches.value_of("threads") {
        args.threads = t.to_string().parse().expect("can't parse threads");
    }

    if let Some(n) = matches.value_of("num-nodes") {
        args.num_nodes = n.to_string().parse().expect("can't parse num-nodes");
    }

    if let Some(s) = matches.value_of("s") {
        args.duration = Duration::new(s.to_string().parse().expect("can't parse duration"), 0);
    }

    if let Some(s) = matches.value_of("tx_count") {
        args.tx_count = s.to_string().parse().expect("can't parse tx_account");
    }

    args.sustained = matches.is_present("sustained");
    args.converge_only = matches.is_present("converge-only");
    args.reject_extra_nodes = matches.is_present("reject-extra-nodes");

    args
}
