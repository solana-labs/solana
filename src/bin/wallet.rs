#[macro_use]
extern crate clap;
extern crate dirs;
#[macro_use]
extern crate solana;

use clap::{App, Arg, ArgMatches, SubCommand};
use solana::client::mk_client;
use solana::drone::DRONE_PORT;
use solana::logger;
use solana::signature::{read_keypair, KeypairUtil};
use solana::thin_client::poll_gossip_for_leader;
use solana::wallet::{parse_command, process_command, WalletConfig, WalletError};
use std::error;
use std::net::SocketAddr;

pub fn parse_args(matches: &ArgMatches) -> Result<WalletConfig, Box<error::Error>> {
    let network = if let Some(addr) = matches.value_of("network") {
        addr.parse().or_else(|_| {
            Err(WalletError::BadParameter(
                "Invalid network location".to_string(),
            ))
        })?
    } else {
        socketaddr!("127.0.0.1:8001")
    };
    let timeout = if let Some(secs) = matches.value_of("timeout") {
        Some(secs.to_string().parse().expect("integer"))
    } else {
        None
    };

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

    let leader = poll_gossip_for_leader(network, timeout)?;

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
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the network at this gossip entry point; defaults to 127.0.0.1:8001"),
        )
        .arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/id.json"),
        ).arg(
            Arg::with_name("timeout")
                .long("timeout")
                .value_name("SECS")
                .takes_value(true)
                .help("Max seconds to wait to get necessary gossip from the network"),
        ).subcommand(
            SubCommand::with_name("airdrop")
                .about("Request a batch of tokens")
                .arg(
                    Arg::with_name("tokens")
                        .long("tokens")
                        .value_name("NUM")
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
                        .value_name("NUM")
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
