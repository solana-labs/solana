#[macro_use]
extern crate clap;
extern crate dirs;
extern crate solana;

use clap::{App, Arg, ArgMatches, SubCommand};
use solana::client::mk_client;
use solana::crdt::NodeInfo;
use solana::drone::DRONE_PORT;
use solana::logger;
use solana::signature::{read_keypair, KeypairUtil};
use solana::thin_client::poll_gossip_for_leader;
use solana::wallet::{parse_command, process_command, read_leader, WalletConfig, WalletError};
use std::error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub fn parse_args(matches: &ArgMatches) -> Result<WalletConfig, Box<error::Error>> {
    let leader: NodeInfo;
    if let Some(l) = matches.value_of("leader") {
        leader = read_leader(l)?.node_info;
    } else {
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        leader = NodeInfo::new_with_socketaddr(&server_addr);
    };
    let timeout: Option<u64>;
    if let Some(secs) = matches.value_of("timeout") {
        timeout = Some(secs.to_string().parse().expect("integer"));
    } else {
        timeout = None;
    }

    let mut path = dirs::home_dir().expect("home directory");
    let id_path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };
    let id = read_keypair(id_path).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Unable to open keypair file: {}",
            err, id_path
        )))
    })?;

    let leader = poll_gossip_for_leader(leader.contact_info.ncp, timeout)?;

    let mut drone_addr = leader.contact_info.tpu;
    drone_addr.set_port(DRONE_PORT);

    let command = parse_command(id.pubkey(), &matches)?;

    Ok(WalletConfig {
        leader,
        id,
        drone_addr, // TODO: Add an option for this.
        command,
    })
}

fn main() -> Result<(), Box<error::Error>> {
    logger::setup();
    let matches = App::new("solana-wallet")
        .version(crate_version!())
        .arg(
            Arg::with_name("leader")
                .short("l")
                .long("leader")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/leader.json"),
        ).arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/id.json"),
        ).arg(
            Arg::with_name("timeout")
                .long("timeout")
                .value_name("SECONDS")
                .takes_value(true)
                .help("Max SECONDS to wait to get necessary gossip from the network"),
        ).subcommand(
            SubCommand::with_name("airdrop")
                .about("Request a batch of tokens")
                .arg(
                    Arg::with_name("tokens")
                        .long("tokens")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .required(true)
                        .help("The number of tokens to request"),
                ),
        ).subcommand(
            SubCommand::with_name("pay")
                .about("Send a payment")
                .arg(
                    Arg::with_name("tokens")
                        .long("tokens")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .required(true)
                        .help("The number of tokens to send"),
                ).arg(
                    Arg::with_name("to")
                        .long("to")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .help("The pubkey of recipient"),
                ),
        ).subcommand(
            SubCommand::with_name("confirm")
                .about("Confirm your payment by signature")
                .arg(
                    Arg::with_name("signature")
                        .index(1)
                        .value_name("SIGNATURE")
                        .required(true)
                        .help("The transaction signature to confirm"),
                ),
        ).subcommand(SubCommand::with_name("balance").about("Get your balance"))
        .subcommand(SubCommand::with_name("address").about("Get your public key"))
        .get_matches();

    let config = parse_args(&matches)?;
    let mut client = mk_client(&config.leader);
    process_command(&config, &mut client)
}
